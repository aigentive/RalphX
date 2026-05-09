use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde::Deserialize;
use tauri::{Emitter, Listener, Manager, Runtime};

use crate::application::agent_conversation_workspace::{
    is_terminal_agent_conversation_publication_status,
    resolve_valid_agent_conversation_workspace_path,
};
use crate::application::agent_conversation_workspace_base::{resolve_workspace_base, BaseStatus};
use crate::application::chat_service::events::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use crate::application::publish_resilience::{
    count_unpublished_publish_commits, inspect_publish_branch_freshness_for_source,
};
use crate::application::{AppState, GitService, TeamService};
use crate::commands::unified_chat_commands::publish_agent_conversation_workspace_for_app_state;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatContextType, ChatConversationId, Project,
};

const AUTO_PUBLISH_FRESHNESS_SCAN_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
struct AgentCompletionPayload {
    conversation_id: String,
    context_type: ChatContextType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AutoPublishFacts {
    has_uncommitted_changes: bool,
    unpublished_commit_count: Option<u32>,
    base_is_ahead: bool,
    base_is_blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPublishTrigger {
    AgentCompletion,
    BaseFreshness,
}

struct AutoPublishGuard {
    conversation_id: String,
}

impl Drop for AutoPublishGuard {
    fn drop(&mut self) {
        auto_publish_in_flight().remove(&self.conversation_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPublishDecision {
    Publish,
    Skip(AutoPublishSkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoPublishSkipReason {
    InactiveWorkspace,
    NotEditWorkspace,
    ExecutionOwnedWorkspace,
    NoExistingPr,
    TerminalPr,
    PublishAlreadyActive,
    NoPendingLocalWork,
    BaseBlocked,
    BaseCurrent,
    AlreadyInFlight,
}

impl AutoPublishSkipReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::InactiveWorkspace => "inactive_workspace",
            Self::NotEditWorkspace => "not_edit_workspace",
            Self::ExecutionOwnedWorkspace => "execution_owned_workspace",
            Self::NoExistingPr => "no_existing_pr",
            Self::TerminalPr => "terminal_pr",
            Self::PublishAlreadyActive => "publish_already_active",
            Self::NoPendingLocalWork => "no_pending_local_work",
            Self::BaseBlocked => "base_blocked",
            Self::BaseCurrent => "base_current",
            Self::AlreadyInFlight => "already_in_flight",
        }
    }
}

/// Register backend-only listeners that continue already-published agent
/// workspace PRs after an agent turn finishes. First-time PR creation remains a
/// deliberate user action; this only updates PRs that already exist.
pub(crate) fn install_agent_workspace_auto_publish_listeners<R>(app: &tauri::App<R>)
where
    R: Runtime,
{
    start_agent_workspace_auto_publish_freshness_scan(app.handle().clone());

    let run_completed_handle = app.handle().clone();
    app.listen_any(AGENT_RUN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            run_completed_handle.clone(),
            AGENT_RUN_COMPLETED,
            event.payload(),
        );
    });

    let turn_completed_handle = app.handle().clone();
    app.listen_any(AGENT_TURN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            turn_completed_handle.clone(),
            AGENT_TURN_COMPLETED,
            event.payload(),
        );
    });
}

pub(crate) fn schedule_agent_workspace_auto_publish_after_freshness_detected<R>(
    app_handle: tauri::AppHandle<R>,
    conversation_id: ChatConversationId,
) where
    R: Runtime,
{
    spawn_auto_publish_existing_pr(
        app_handle,
        "agent_workspace_freshness",
        AutoPublishTrigger::BaseFreshness,
        conversation_id,
    );
}

fn spawn_auto_publish_from_completion_event<R>(
    app_handle: tauri::AppHandle<R>,
    event_name: &'static str,
    payload: &str,
) where
    R: Runtime,
{
    let payload = match serde_json::from_str::<AgentCompletionPayload>(payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                event_name,
                error = %error,
                "Skipping agent workspace auto-publish: completion payload could not be parsed"
            );
            return;
        }
    };

    if payload.context_type != ChatContextType::Project {
        return;
    }

    let conversation_id = ChatConversationId::from_string(payload.conversation_id);
    spawn_auto_publish_existing_pr(
        app_handle,
        event_name,
        AutoPublishTrigger::AgentCompletion,
        conversation_id,
    );
}

fn spawn_auto_publish_existing_pr<R>(
    app_handle: tauri::AppHandle<R>,
    event_name: &'static str,
    trigger: AutoPublishTrigger,
    conversation_id: ChatConversationId,
) where
    R: Runtime,
{
    let Some(_guard) = begin_auto_publish(&conversation_id) else {
        tracing::debug!(
            event_name,
            conversation_id = conversation_id.as_str(),
            reason = AutoPublishSkipReason::AlreadyInFlight.as_str(),
            "Skipped agent workspace auto-publish"
        );
        return;
    };

    tauri::async_runtime::spawn(async move {
        let _guard = _guard;
        match auto_publish_existing_agent_workspace_pr_from_app_handle(
            &app_handle,
            conversation_id,
            trigger,
        )
        .await
        {
            Ok(AutoPublishDecision::Publish) => {}
            Ok(AutoPublishDecision::Skip(reason)) => {
                tracing::debug!(
                    event_name,
                    reason = reason.as_str(),
                    "Skipped agent workspace auto-publish"
                );
            }
            Err(error) => {
                tracing::warn!(
                    event_name,
                    error = %error,
                    "Agent workspace auto-publish failed"
                );
            }
        }
    });
}

fn start_agent_workspace_auto_publish_freshness_scan<R>(app_handle: tauri::AppHandle<R>)
where
    R: Runtime,
{
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(AUTO_PUBLISH_FRESHNESS_SCAN_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            match auto_publish_stale_published_agent_workspace_prs_from_app_handle(&app_handle)
                .await
            {
                Ok(0) => {}
                Ok(count) => {
                    tracing::info!(
                        count,
                        "Agent workspace auto-publish freshness scan published stale PR workspaces"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Agent workspace auto-publish freshness scan failed"
                    );
                }
            }
        }
    });
}

async fn auto_publish_stale_published_agent_workspace_prs_from_app_handle<R>(
    app_handle: &tauri::AppHandle<R>,
) -> Result<usize, String>
where
    R: Runtime,
{
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    if state.startup_git_auth_recovery_state.is_pending() {
        return Ok(0);
    }
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()
        .ok_or_else(|| "ExecutionState is not available".to_string())?
        .inner()
        .clone();
    let team_service = app_handle
        .try_state::<Arc<TeamService>>()
        .map(|state| state.inner().clone());

    let workspaces = state
        .agent_conversation_workspace_repo
        .list_active_direct_published_workspaces()
        .await
        .map_err(|error| error.to_string())?;
    let mut published = 0;

    for workspace in workspaces {
        let conversation_id = workspace.conversation_id.clone();
        let Some(_guard) = begin_auto_publish(&conversation_id) else {
            continue;
        };

        match auto_publish_existing_agent_workspace_pr(
            state.inner(),
            &execution_state,
            team_service.clone(),
            Some(app_handle.clone()),
            conversation_id,
            AutoPublishTrigger::BaseFreshness,
        )
        .await
        {
            Ok(AutoPublishDecision::Publish) => {
                published += 1;
            }
            Ok(AutoPublishDecision::Skip(reason)) => {
                tracing::debug!(
                    conversation_id = workspace.conversation_id.as_str(),
                    reason = reason.as_str(),
                    "Skipped stale-base agent workspace auto-publish"
                );
            }
            Err(error) => {
                tracing::warn!(
                    conversation_id = workspace.conversation_id.as_str(),
                    error = %error,
                    "Stale-base agent workspace auto-publish failed"
                );
            }
        }
    }

    Ok(published)
}

async fn auto_publish_existing_agent_workspace_pr_from_app_handle(
    app_handle: &tauri::AppHandle<impl Runtime>,
    conversation_id: ChatConversationId,
    trigger: AutoPublishTrigger,
) -> Result<AutoPublishDecision, String> {
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()
        .ok_or_else(|| "ExecutionState is not available".to_string())?
        .inner()
        .clone();
    let team_service = app_handle
        .try_state::<Arc<TeamService>>()
        .map(|state| state.inner().clone());

    auto_publish_existing_agent_workspace_pr(
        state.inner(),
        &execution_state,
        team_service,
        Some(app_handle.clone()),
        conversation_id,
        trigger,
    )
    .await
}

async fn auto_publish_existing_agent_workspace_pr<R>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    team_service: Option<Arc<TeamService>>,
    app_handle: Option<tauri::AppHandle<R>>,
    conversation_id: ChatConversationId,
    trigger: AutoPublishTrigger,
) -> Result<AutoPublishDecision, String>
where
    R: Runtime,
{
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::NoExistingPr,
        ));
    };

    if let Some(reason) = static_auto_publish_skip_reason(&workspace) {
        return Ok(AutoPublishDecision::Skip(reason));
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let worktree_path = resolve_valid_agent_conversation_workspace_path(&project, &workspace)
        .await
        .map_err(|error| error.to_string())?;
    let facts = collect_auto_publish_facts(&project, &workspace, worktree_path).await?;
    let decision = should_auto_publish_existing_pr(&workspace, facts, trigger);
    if decision != AutoPublishDecision::Publish {
        return Ok(decision);
    }

    tracing::info!(
        conversation_id = %workspace.conversation_id,
        pr_number = workspace.publication_pr_number,
        "Auto-publishing existing agent workspace PR after agent completion"
    );
    let result = publish_agent_conversation_workspace_for_app_state(
        state,
        execution_state,
        team_service,
        conversation_id,
        true,
    )
    .await;

    if let Some(app_handle) = app_handle.as_ref() {
        let _ = app_handle.emit(
            "agent:workspace_changed",
            serde_json::json!({ "conversation_id": conversation_id.as_str() }),
        );
    }

    if result.is_ok() {
        return Ok(AutoPublishDecision::Publish);
    }

    if publish_was_routed_to_agent_repair(state, &conversation_id).await? {
        tracing::info!(
            conversation_id = %workspace.conversation_id,
            "Auto-publish routed existing agent workspace PR through repair agent"
        );
        return Ok(AutoPublishDecision::Publish);
    }

    result.map(|_| AutoPublishDecision::Publish)
}

async fn publish_was_routed_to_agent_repair(
    state: &AppState,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    Ok(state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .and_then(|workspace| workspace.publication_push_status)
        .as_deref()
        == Some("needs_agent"))
}

async fn collect_auto_publish_facts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    worktree_path: PathBuf,
) -> Result<AutoPublishFacts, String> {
    let has_uncommitted_changes = GitService::has_uncommitted_changes(&worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    let unpublished_commit_count =
        count_unpublished_publish_commits(&worktree_path, &workspace.branch_name)
            .await
            .map_err(|error| error.to_string())?;
    let base_resolution = resolve_workspace_base(project, workspace)
        .await
        .map_err(|error| error.to_string())?;
    let base_is_blocked = base_resolution.status == BaseStatus::Blocked;
    let base_is_ahead = if base_is_blocked {
        false
    } else {
        let effective_base_ref = base_resolution
            .effective_checkout_ref()
            .map_err(|error| error.to_string())?;
        inspect_publish_branch_freshness_for_source(
            &worktree_path,
            effective_base_ref,
            &workspace.branch_name,
            workspace.base_commit.as_deref(),
        )
        .await
        .map_err(|error| error.to_string())?
        .is_base_ahead
    };

    Ok(AutoPublishFacts {
        has_uncommitted_changes,
        unpublished_commit_count,
        base_is_ahead,
        base_is_blocked,
    })
}

fn should_auto_publish_existing_pr(
    workspace: &AgentConversationWorkspace,
    facts: AutoPublishFacts,
    trigger: AutoPublishTrigger,
) -> AutoPublishDecision {
    if let Some(reason) = static_auto_publish_skip_reason(workspace) {
        return AutoPublishDecision::Skip(reason);
    }
    if facts.base_is_blocked {
        return AutoPublishDecision::Skip(AutoPublishSkipReason::BaseBlocked);
    }
    if trigger == AutoPublishTrigger::BaseFreshness && !facts.base_is_ahead {
        return AutoPublishDecision::Skip(AutoPublishSkipReason::BaseCurrent);
    }
    if !facts.base_is_ahead
        && !facts.has_uncommitted_changes
        && facts.unpublished_commit_count.unwrap_or(0) == 0
    {
        return AutoPublishDecision::Skip(AutoPublishSkipReason::NoPendingLocalWork);
    }

    AutoPublishDecision::Publish
}

fn static_auto_publish_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<AutoPublishSkipReason> {
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Some(AutoPublishSkipReason::InactiveWorkspace);
    }
    if workspace.mode != AgentConversationWorkspaceMode::Edit {
        return Some(AutoPublishSkipReason::NotEditWorkspace);
    }
    if workspace.is_execution_owned() {
        return Some(AutoPublishSkipReason::ExecutionOwnedWorkspace);
    }
    if workspace.publication_pr_number.is_none() {
        return Some(AutoPublishSkipReason::NoExistingPr);
    }
    if is_terminal_agent_conversation_publication_status(workspace.publication_pr_status.as_deref())
    {
        return Some(AutoPublishSkipReason::TerminalPr);
    }
    if workspace
        .publication_push_status
        .as_deref()
        .is_some_and(is_active_publish_status)
    {
        return Some(AutoPublishSkipReason::PublishAlreadyActive);
    }

    None
}

fn is_active_publish_status(status: &str) -> bool {
    matches!(
        status,
        "checking" | "committing" | "refreshing" | "describing" | "pushing" | "needs_agent"
    )
}

fn auto_publish_in_flight() -> &'static DashMap<String, ()> {
    static AUTO_PUBLISH_IN_FLIGHT: OnceLock<DashMap<String, ()>> = OnceLock::new();
    AUTO_PUBLISH_IN_FLIGHT.get_or_init(DashMap::new)
}

fn begin_auto_publish(conversation_id: &ChatConversationId) -> Option<AutoPublishGuard> {
    let conversation_id = conversation_id.as_str().to_string();
    match auto_publish_in_flight().entry(conversation_id.clone()) {
        Entry::Occupied(_) => None,
        Entry::Vacant(entry) => {
            entry.insert(());
            Some(AutoPublishGuard { conversation_id })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{IdeationAnalysisBaseRefKind, PlanBranchId, ProjectId};
    use std::path::Path;
    use std::process::Command;
    use tauri::test::{mock_builder, mock_context, noop_assets, MockRuntime};

    fn mock_app(state: AppState, execution_state: Arc<ExecutionState>) -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(state)
            .manage(execution_state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    fn mock_app_with_state(state: AppState) -> tauri::App<MockRuntime> {
        mock_builder()
            .manage(state)
            .build(mock_context(noop_assets()))
            .expect("mock app should build")
    }

    async fn wait_for_spawned_auto_publish() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    fn workspace() -> AgentConversationWorkspace {
        let mut workspace = AgentConversationWorkspace::new(
            ChatConversationId::from_string("11111111-1111-1111-1111-111111111111"),
            ProjectId::from_string("project-1".to_string()),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("0".repeat(40)),
            "ralphx/test/agent-workspace".to_string(),
            "/tmp/ralphx-agent-workspace".to_string(),
        );
        workspace.publication_pr_number = Some(42);
        workspace.publication_pr_status = Some("open".to_string());
        workspace.publication_push_status = Some("pushed".to_string());
        workspace
    }

    fn facts() -> AutoPublishFacts {
        AutoPublishFacts {
            has_uncommitted_changes: false,
            unpublished_commit_count: Some(0),
            base_is_ahead: false,
            base_is_blocked: false,
        }
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

    fn git_workspace_fixture() -> (tempfile::TempDir, Project, AgentConversationWorkspace) {
        let root = tempfile::tempdir().expect("temp repo should be created");
        let project_repo = root.path().join("project");
        let worktree_parent = root.path().join("worktrees");
        std::fs::create_dir_all(&project_repo).expect("project repo directory should be created");
        std::fs::create_dir_all(&worktree_parent).expect("worktree parent should be created");

        run_git(&project_repo, &["init"]);
        run_git(&project_repo, &["config", "user.email", "test@example.com"]);
        run_git(&project_repo, &["config", "user.name", "Test User"]);
        run_git(&project_repo, &["checkout", "-b", "main"]);
        std::fs::write(project_repo.join("README.md"), "initial\n")
            .expect("fixture file should be written");
        run_git(&project_repo, &["add", "README.md"]);
        run_git(&project_repo, &["commit", "-m", "initial"]);
        let base_commit = run_git(&project_repo, &["rev-parse", "HEAD"]);

        let mut workspace = workspace();
        let mut project = Project::new(
            "Auto Publish Fixture".to_string(),
            project_repo.to_string_lossy().to_string(),
        );
        project.id = workspace.project_id.clone();
        project.base_branch = Some("main".to_string());
        project.worktree_parent_directory = Some(worktree_parent.to_string_lossy().to_string());
        workspace.base_ref = "main".to_string();
        workspace.base_display_name = Some("main".to_string());
        workspace.base_commit = Some(base_commit);
        workspace.branch_name = "ralphx/test/agent-workspace".to_string();
        let worktree_path = crate::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path(
            &project,
            &workspace.conversation_id,
        )
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

    #[test]
    fn skip_reason_strings_are_stable_for_logs() {
        let cases = [
            (
                AutoPublishSkipReason::InactiveWorkspace,
                "inactive_workspace",
            ),
            (
                AutoPublishSkipReason::NotEditWorkspace,
                "not_edit_workspace",
            ),
            (
                AutoPublishSkipReason::ExecutionOwnedWorkspace,
                "execution_owned_workspace",
            ),
            (AutoPublishSkipReason::NoExistingPr, "no_existing_pr"),
            (AutoPublishSkipReason::TerminalPr, "terminal_pr"),
            (
                AutoPublishSkipReason::PublishAlreadyActive,
                "publish_already_active",
            ),
            (
                AutoPublishSkipReason::NoPendingLocalWork,
                "no_pending_local_work",
            ),
            (AutoPublishSkipReason::BaseBlocked, "base_blocked"),
            (AutoPublishSkipReason::BaseCurrent, "base_current"),
            (AutoPublishSkipReason::AlreadyInFlight, "already_in_flight"),
        ];

        for (reason, expected) in cases {
            assert_eq!(reason.as_str(), expected);
        }
    }

    #[test]
    fn static_preflight_skips_inactive_workspace() {
        let mut workspace = workspace();
        workspace.status = AgentConversationWorkspaceStatus::Archived;

        assert_eq!(
            static_auto_publish_skip_reason(&workspace),
            Some(AutoPublishSkipReason::InactiveWorkspace)
        );
    }

    #[test]
    fn static_preflight_skips_non_edit_workspace() {
        let mut workspace = workspace();
        workspace.mode = AgentConversationWorkspaceMode::Chat;

        assert_eq!(
            static_auto_publish_skip_reason(&workspace),
            Some(AutoPublishSkipReason::NotEditWorkspace)
        );
    }

    #[test]
    fn static_preflight_skips_execution_owned_workspace() {
        let mut workspace = workspace();
        workspace.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-1".to_string()));

        assert_eq!(
            static_auto_publish_skip_reason(&workspace),
            Some(AutoPublishSkipReason::ExecutionOwnedWorkspace)
        );
    }

    #[test]
    fn static_preflight_skips_terminal_pr() {
        let mut workspace = workspace();
        workspace.publication_pr_status = Some("merged".to_string());

        assert_eq!(
            static_auto_publish_skip_reason(&workspace),
            Some(AutoPublishSkipReason::TerminalPr)
        );
    }

    #[test]
    fn active_publish_statuses_lock_auto_publish() {
        for status in [
            "checking",
            "committing",
            "refreshing",
            "describing",
            "pushing",
            "needs_agent",
        ] {
            assert!(is_active_publish_status(status));
        }
        assert!(!is_active_publish_status("pushed"));
        assert!(!is_active_publish_status("failed"));
    }

    #[test]
    fn in_flight_guard_serializes_by_conversation_id() {
        let conversation_id =
            ChatConversationId::from_string("22222222-2222-2222-2222-222222222222");
        let guard = begin_auto_publish(&conversation_id).expect("first guard should enter");

        assert!(begin_auto_publish(&conversation_id).is_none());

        drop(guard);
        assert!(begin_auto_publish(&conversation_id).is_some());
    }

    #[test]
    fn spawn_auto_publish_skips_when_already_in_flight() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        let conversation_id =
            ChatConversationId::from_string("33333333-3333-3333-3333-333333333333");
        let _guard = begin_auto_publish(&conversation_id).expect("guard should enter");

        spawn_auto_publish_existing_pr(
            app.handle().clone(),
            "test_event",
            AutoPublishTrigger::AgentCompletion,
            conversation_id,
        );
    }

    #[test]
    fn malformed_completion_payload_is_ignored() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        spawn_auto_publish_from_completion_event(app.handle().clone(), "test_event", "{not-json");
    }

    #[test]
    fn non_project_completion_payload_is_ignored() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");

        spawn_auto_publish_from_completion_event(
            app.handle().clone(),
            "test_event",
            r#"{"conversation_id":"33333333-3333-3333-3333-333333333333","context_type":"task"}"#,
        );
    }

    #[tokio::test]
    async fn project_completion_payload_schedules_auto_publish_task() {
        let app = mock_app(AppState::new_test(), Arc::new(ExecutionState::new()));

        spawn_auto_publish_from_completion_event(
            app.handle().clone(),
            "test_event",
            r#"{"conversation_id":"44444444-4444-4444-4444-444444444444","context_type":"project"}"#,
        );

        wait_for_spawned_auto_publish().await;
    }

    #[tokio::test]
    async fn freshness_detected_schedules_auto_publish_task() {
        let app = mock_app(AppState::new_test(), Arc::new(ExecutionState::new()));

        schedule_agent_workspace_auto_publish_after_freshness_detected(
            app.handle().clone(),
            ChatConversationId::from_string("55555555-5555-5555-5555-555555555555"),
        );

        wait_for_spawned_auto_publish().await;
    }

    #[tokio::test]
    async fn installed_listeners_handle_completion_events() {
        let app = mock_app(AppState::new_test(), Arc::new(ExecutionState::new()));
        install_agent_workspace_auto_publish_listeners(&app);

        app.emit(
            AGENT_RUN_COMPLETED,
            serde_json::json!({
                "conversation_id": "66666666-6666-6666-6666-666666666666",
                "context_type": "project"
            }),
        )
        .expect("run completion event should emit");
        app.emit(
            AGENT_TURN_COMPLETED,
            serde_json::json!({
                "conversation_id": "77777777-7777-7777-7777-777777777777",
                "context_type": "project"
            }),
        )
        .expect("turn completion event should emit");

        wait_for_spawned_auto_publish().await;
    }

    #[test]
    fn auto_publish_requires_existing_pr() {
        let mut workspace = workspace();
        workspace.publication_pr_number = None;

        let decision = should_auto_publish_existing_pr(
            &workspace,
            AutoPublishFacts {
                has_uncommitted_changes: true,
                unpublished_commit_count: Some(0),
                base_is_ahead: false,
                base_is_blocked: false,
            },
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoExistingPr)
        );
    }

    #[test]
    fn auto_publish_runs_for_existing_pr_with_uncommitted_changes() {
        let mut facts = facts();
        facts.has_uncommitted_changes = true;
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            facts,
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(decision, AutoPublishDecision::Publish);
    }

    #[test]
    fn auto_publish_runs_for_existing_pr_with_unpublished_commits() {
        let mut facts = facts();
        facts.unpublished_commit_count = Some(2);
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            facts,
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(decision, AutoPublishDecision::Publish);
    }

    #[test]
    fn auto_publish_skips_existing_pr_without_pending_local_work() {
        let decision = should_auto_publish_existing_pr(
            &workspace(),
            facts(),
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoPendingLocalWork)
        );
    }

    #[test]
    fn auto_publish_skips_when_publish_or_repair_already_active() {
        let mut workspace = workspace();
        workspace.publication_push_status = Some("needs_agent".to_string());

        let decision = should_auto_publish_existing_pr(
            &workspace,
            AutoPublishFacts {
                has_uncommitted_changes: true,
                unpublished_commit_count: Some(0),
                base_is_ahead: false,
                base_is_blocked: false,
            },
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::PublishAlreadyActive)
        );
    }

    #[test]
    fn auto_publish_runs_for_existing_pr_with_stale_base_without_local_work() {
        let mut facts = facts();
        facts.base_is_ahead = true;
        let decision =
            should_auto_publish_existing_pr(&workspace(), facts, AutoPublishTrigger::BaseFreshness);

        assert_eq!(decision, AutoPublishDecision::Publish);
    }

    #[test]
    fn freshness_scan_skips_existing_pr_when_base_is_current() {
        let mut facts = facts();
        facts.has_uncommitted_changes = true;
        facts.unpublished_commit_count = Some(1);
        let decision =
            should_auto_publish_existing_pr(&workspace(), facts, AutoPublishTrigger::BaseFreshness);

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::BaseCurrent)
        );
    }

    #[test]
    fn auto_publish_skips_blocked_base() {
        let mut facts = facts();
        facts.base_is_blocked = true;

        let decision = should_auto_publish_existing_pr(
            &workspace(),
            facts,
            AutoPublishTrigger::AgentCompletion,
        );

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::BaseBlocked)
        );
    }

    #[tokio::test]
    async fn app_handle_auto_publish_errors_without_state() {
        let app = mock_builder()
            .build(mock_context(noop_assets()))
            .expect("mock app should build");
        let error = auto_publish_existing_agent_workspace_pr_from_app_handle(
            app.handle(),
            ChatConversationId::from_string("44444444-4444-4444-4444-444444444444"),
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect_err("missing state should fail");

        assert_eq!(error, "AppState is not available");
    }

    #[tokio::test]
    async fn app_handle_auto_publish_errors_without_execution_state() {
        let app = mock_app_with_state(AppState::new_test());
        let error = auto_publish_existing_agent_workspace_pr_from_app_handle(
            app.handle(),
            ChatConversationId::from_string("88888888-8888-8888-8888-888888888888"),
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect_err("missing execution state should fail");

        assert_eq!(error, "ExecutionState is not available");
    }

    #[tokio::test]
    async fn app_handle_auto_publish_skips_when_workspace_is_missing() {
        let app = mock_app(AppState::new_test(), Arc::new(ExecutionState::new()));

        let decision = auto_publish_existing_agent_workspace_pr_from_app_handle(
            app.handle(),
            ChatConversationId::from_string("55555555-5555-5555-5555-555555555555"),
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect("missing workspace should be a skip");

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoExistingPr)
        );
    }

    #[tokio::test]
    async fn app_handle_freshness_scan_skips_when_startup_git_auth_is_pending() {
        let state = AppState::new_test();
        state.startup_git_auth_recovery_state.mark_pending();
        let app = mock_app(state, Arc::new(ExecutionState::new()));

        let count = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect("pending startup recovery should skip scan");

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn app_handle_freshness_scan_errors_without_execution_state() {
        let app = mock_app_with_state(AppState::new_test());
        let error = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect_err("missing execution state should fail");

        assert_eq!(error, "ExecutionState is not available");
    }

    #[tokio::test]
    async fn app_handle_freshness_scan_returns_zero_without_workspaces() {
        let app = mock_app(AppState::new_test(), Arc::new(ExecutionState::new()));

        let count = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect("empty workspace set should scan successfully");

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn app_handle_freshness_scan_skips_current_base_workspace() {
        let (_repo, project, workspace) = git_workspace_fixture();
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
        let app = mock_app(state, Arc::new(ExecutionState::new()));

        let count = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect("current-base workspace should be skipped");

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn app_handle_freshness_scan_skips_in_flight_workspace() {
        let state = AppState::new_test();
        let workspace = workspace();
        let conversation_id = workspace.conversation_id.clone();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should seed");
        let _guard = begin_auto_publish(&conversation_id).expect("guard should enter");
        let app = mock_app(state, Arc::new(ExecutionState::new()));

        let count = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect("in-flight workspace should be skipped");

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn freshness_scan_continues_past_workspace_errors() {
        let state = AppState::new_test();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace())
            .await
            .expect("workspace should seed");
        let app = mock_app(state, Arc::new(ExecutionState::new()));

        let count = auto_publish_stale_published_agent_workspace_prs_from_app_handle(app.handle())
            .await
            .expect("workspace-level errors should be logged and skipped");

        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn direct_auto_publish_reports_missing_project() {
        let state = AppState::new_test();
        let workspace = workspace();
        let conversation_id = workspace.conversation_id.clone();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should seed");
        let execution_state = Arc::new(ExecutionState::new());

        let error = auto_publish_existing_agent_workspace_pr::<MockRuntime>(
            &state,
            &execution_state,
            None,
            None,
            conversation_id,
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect_err("missing project should fail");

        assert!(error.contains("Project not found: project-1"));
    }

    #[tokio::test]
    async fn direct_auto_publish_skips_valid_current_base_without_local_work() {
        let (_repo, project, workspace) = git_workspace_fixture();
        let state = AppState::new_test();
        let conversation_id = workspace.conversation_id.clone();
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
        let execution_state = Arc::new(ExecutionState::new());

        let decision = auto_publish_existing_agent_workspace_pr::<MockRuntime>(
            &state,
            &execution_state,
            None,
            None,
            conversation_id,
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect("current-base workspace should skip");

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::NoPendingLocalWork)
        );
    }

    #[tokio::test]
    async fn collect_auto_publish_facts_reports_blocked_base() {
        let (_repo, project, mut workspace) = git_workspace_fixture();
        workspace.base_ref = "deleted-base".to_string();
        workspace.base_commit = None;

        let facts = collect_auto_publish_facts(
            &project,
            &workspace,
            PathBuf::from(&workspace.worktree_path),
        )
        .await
        .expect("blocked base should still collect facts");

        assert!(facts.base_is_blocked);
        assert!(!facts.base_is_ahead);
    }

    #[tokio::test]
    async fn repair_routing_check_reads_needs_agent_status() {
        let state = AppState::new_test();
        let conversation_id =
            ChatConversationId::from_string("66666666-6666-6666-6666-666666666666");

        assert!(
            !publish_was_routed_to_agent_repair(&state, &conversation_id)
                .await
                .expect("missing workspace should not be routed")
        );

        let mut workspace = workspace();
        workspace.conversation_id = conversation_id.clone();
        workspace.publication_push_status = Some("needs_agent".to_string());
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should seed");

        assert!(publish_was_routed_to_agent_repair(&state, &conversation_id)
            .await
            .expect("needs_agent workspace should be routed"));
    }

    #[tokio::test]
    async fn direct_auto_publish_static_skip_does_not_resolve_project() {
        let state = AppState::new_test();
        let mut workspace = workspace();
        workspace.status = AgentConversationWorkspaceStatus::Missing;
        let conversation_id = workspace.conversation_id.clone();
        state
            .agent_conversation_workspace_repo
            .create_or_update(workspace)
            .await
            .expect("workspace should seed");
        let execution_state = Arc::new(ExecutionState::new());

        let decision = auto_publish_existing_agent_workspace_pr::<MockRuntime>(
            &state,
            &execution_state,
            None,
            None,
            conversation_id,
            AutoPublishTrigger::AgentCompletion,
        )
        .await
        .expect("static skip should not need a project");

        assert_eq!(
            decision,
            AutoPublishDecision::Skip(AutoPublishSkipReason::InactiveWorkspace)
        );
    }
}
