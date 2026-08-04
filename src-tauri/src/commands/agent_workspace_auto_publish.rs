use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use serde::Deserialize;
use tauri::{Emitter, Listener, Manager, Runtime};
use uuid::Uuid;

use crate::application::agent_conversation_workspace::is_terminal_agent_conversation_publication_status;
use crate::application::agent_conversation_workspace_base::{resolve_workspace_base, BaseStatus};
use crate::application::agent_workspace_pr_autofix_attempt::{
    is_exact_pr_autofix_run_for_pr, load_latest_exact_pr_autofix_run_for_pr,
};
use crate::application::agent_workspace_pr_supervision_recovery::{
    build_agent_workspace_pr_supervision_recovery_deps,
    schedule_agent_workspace_pr_supervision_recovery, AgentWorkspacePrSupervisionRecoveryTrigger,
};
use crate::application::agent_workspace_publish_lease::{
    begin_publish_operation_scope, publish_operation_lease_is_live,
    spawn_publish_operation_lease_heartbeat_for_scope, stop_publish_operation_lease_heartbeat,
};
use crate::application::agent_workspace_publish_recovery::is_blocked_and_not_auto_retryable;
use crate::application::agent_workspace_publish_recovery::{
    AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS, AGENT_WORKSPACE_PUBLISH_REDRIVE_REQUESTED,
};
use crate::application::chat_service::events::{AGENT_RUN_COMPLETED, AGENT_TURN_COMPLETED};
use crate::application::git_service::git_cmd;
use crate::application::publish_resilience::{
    count_publishable_commits_with_base_fallback, count_unpublished_publish_commits,
    inspect_publish_branch_freshness_for_source_after_fetch,
};
use crate::application::{AppState, GitService};
use crate::commands::unified_chat_commands::{
    install_agent_workspace_repair_publish_continuation,
    publish_agent_conversation_workspace_for_app_state, resolve_agent_workspace_publish_target,
    AgentConversationWorkspacePublishTarget, AgentWorkspacePrFixReviewPublishCommandResumer,
};
use crate::commands::ExecutionState;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRunId, ChatContextType, ChatConversationId, Project,
};
use crate::infrastructure::git_auth::{inspect_repository_capability, RepositoryCapability};

const AUTO_PUBLISH_FRESHNESS_SCAN_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Debug, Deserialize)]
pub(crate) struct AgentCompletionPayload {
    pub(crate) conversation_id: String,
    pub(crate) context_type: ChatContextType,
    pub(crate) run_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoPublishFacts {
    pub(crate) has_uncommitted_changes: bool,
    pub(crate) unpublished_commit_count: Option<u32>,
    pub(crate) base_is_ahead: bool,
    pub(crate) base_is_blocked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPublishTrigger {
    AgentCompletion,
    BaseFreshness,
}

pub(crate) struct AutoPublishGuard {
    conversation_id: String,
}

impl Drop for AutoPublishGuard {
    fn drop(&mut self) {
        auto_publish_in_flight().remove(&self.conversation_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPublishDecision {
    Publish,
    Skip(AutoPublishSkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoPublishSkipReason {
    WorkspaceMissing,
    InactiveWorkspace,
    NotEditWorkspace,
    ExecutionOwnedWorkspace,
    NoExistingPr,
    InitialPrAutoPublishDisabled,
    AutoPublishDisabled,
    TerminalPr,
    PublishAlreadyActive,
    NoPendingLocalWork,
    BaseBlocked,
    BaseCurrent,
    NewPrUnavailable,
    AlreadyInFlight,
    DurableRepairBlockedExhausted,
}

impl AutoPublishSkipReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceMissing => "workspace_missing",
            Self::InactiveWorkspace => "inactive_workspace",
            Self::NotEditWorkspace => "not_edit_workspace",
            Self::ExecutionOwnedWorkspace => "execution_owned_workspace",
            Self::NoExistingPr => "no_existing_pr",
            Self::InitialPrAutoPublishDisabled => "initial_pr_auto_publish_disabled",
            Self::AutoPublishDisabled => "auto_publish_disabled",
            Self::TerminalPr => "terminal_pr",
            Self::PublishAlreadyActive => "publish_already_active",
            Self::NoPendingLocalWork => "no_pending_local_work",
            Self::BaseBlocked => "base_blocked",
            Self::BaseCurrent => "base_current",
            Self::NewPrUnavailable => "new_pr_unavailable",
            Self::AlreadyInFlight => "already_in_flight",
            Self::DurableRepairBlockedExhausted => "durable_repair_blocked_exhausted",
        }
    }
}

/// Register backend-only listeners that publish opted-in agent workspaces after
/// an agent turn finishes, then keep already-published PRs fresh.
pub(crate) fn install_agent_workspace_auto_publish_listeners<R>(app_handle: tauri::AppHandle<R>)
where
    R: Runtime,
{
    if let Some(state) = app_handle.try_state::<AppState>() {
        install_agent_workspace_repair_publish_continuation(state.inner());
    }
    start_agent_workspace_auto_publish_freshness_scan(app_handle.clone());

    let run_completed_handle = app_handle.clone();
    app_handle.listen_any(AGENT_RUN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            run_completed_handle.clone(),
            AGENT_RUN_COMPLETED,
            event.payload(),
        );
        spawn_pr_supervision_recovery_from_run_completed_event(
            run_completed_handle.clone(),
            event.payload(),
        );
    });

    let turn_completed_handle = app_handle.clone();
    app_handle.listen_any(AGENT_TURN_COMPLETED, move |event| {
        spawn_auto_publish_from_completion_event(
            turn_completed_handle.clone(),
            AGENT_TURN_COMPLETED,
            event.payload(),
        );
    });

    let recovery_redrive_handle = app_handle.clone();
    app_handle.listen_any(AGENT_WORKSPACE_PUBLISH_REDRIVE_REQUESTED, move |event| {
        spawn_auto_publish_from_recovery_event(recovery_redrive_handle.clone(), event.payload());
    });
}

fn spawn_auto_publish_from_recovery_event<R>(app_handle: tauri::AppHandle<R>, payload: &str)
where
    R: Runtime,
{
    let conversation_id = match serde_json::from_str::<String>(payload) {
        Ok(conversation_id) if !conversation_id.trim().is_empty() => {
            ChatConversationId::from_string(conversation_id)
        }
        Ok(_) => return,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Skipping recovered agent workspace publication re-drive: event payload could not be parsed"
            );
            return;
        }
    };
    spawn_auto_publish_existing_pr(
        app_handle,
        AGENT_WORKSPACE_PUBLISH_REDRIVE_REQUESTED,
        AutoPublishTrigger::BaseFreshness,
        conversation_id,
    );
}

fn spawn_pr_supervision_recovery_from_run_completed_event<R>(
    app_handle: tauri::AppHandle<R>,
    payload: &str,
) where
    R: Runtime,
{
    let payload = match serde_json::from_str::<AgentCompletionPayload>(payload) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Skipping agent workspace PR supervision recovery: completion payload could not be parsed"
            );
            return;
        }
    };
    if payload.context_type != ChatContextType::Project {
        return;
    }
    let Some(run_id) = payload
        .run_id
        .as_deref()
        .and_then(|value| value.parse::<AgentRunId>().ok())
    else {
        return;
    };
    let conversation_id = ChatConversationId::from_string(payload.conversation_id.clone());

    tauri::async_runtime::spawn(async move {
        let Some(state) = app_handle.try_state::<AppState>() else {
            tracing::warn!(
                "Skipping agent workspace PR supervision recovery: AppState is not available"
            );
            return;
        };
        let run_id_string = run_id.to_string();
        match state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
        {
            Ok(Some(workspace))
                if workspace.publish_lease_owner_run_id.as_deref()
                    == Some(run_id_string.as_str()) =>
            {
                if let Some(token) = workspace.publish_lease_token.as_deref() {
                    let release = state
                        .agent_conversation_workspace_repo
                        .release_publish_lease(
                            &conversation_id,
                            token,
                            Some("refreshed"),
                            chrono::Utc::now(),
                        )
                        .await;
                    stop_publish_operation_lease_heartbeat(&conversation_id, token);
                    if let Err(error) = release {
                        tracing::warn!(conversation_id = %conversation_id, agent_run_id = %run_id, error = %error, "Failed to settle completed agent publish lease");
                    }
                }
            }
            Ok(_) => {}
            Err(error) => {
                tracing::warn!(conversation_id = %conversation_id, agent_run_id = %run_id, error = %error, "Failed to inspect completed agent publish lease")
            }
        }
        match crate::application::agent_workspace_publish_recovery::
            recover_agent_workspace_repair_after_terminal_run(state.inner(), &conversation_id, &run_id)
            .await
        {
            Ok(true) => tracing::info!(
                conversation_id = conversation_id.as_str(),
                agent_run_id = %run_id,
                "Recovered durable workspace repair after terminal agent run"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                conversation_id = conversation_id.as_str(),
                agent_run_id = %run_id,
                error = %error,
                "Durable workspace repair terminal recovery failed closed"
            ),
        }
        match is_exact_terminal_pr_autofix_completion(state.inner(), &payload).await {
            Ok(true) => {
                let Some(deps) = build_pr_supervision_recovery_deps_from_managed_state(&app_handle)
                else {
                    tracing::warn!(
                        "Skipping agent workspace PR supervision recovery: runtime services are not available"
                    );
                    return;
                };
                schedule_agent_workspace_pr_supervision_recovery(
                    deps,
                    conversation_id,
                    AgentWorkspacePrSupervisionRecoveryTrigger::AgentRunCompleted,
                    false,
                );
            }
            Ok(false) => {}
            Err(error) => tracing::warn!(
                error = %error,
                "Skipping agent workspace PR supervision recovery after an unverified completion event"
            ),
        }
    });
}

fn build_pr_supervision_recovery_deps_from_managed_state<R>(
    app_handle: &tauri::AppHandle<R>,
) -> Option<crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrSupervisionRecoveryDeps>
where
    R: Runtime,
{
    let state = app_handle.try_state::<AppState>()?;
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()?
        .inner()
        .clone();
    let runtime_app_handle = state.app_handle.clone();
    let transition_service = Arc::new(state.build_transition_service_for_runtime(
        Arc::clone(&execution_state),
        runtime_app_handle.clone(),
    ));
    let chat_service: Arc<dyn crate::application::ChatService> =
        Arc::new(state.build_chat_service_for_runtime(
            Some(execution_state.clone()),
            runtime_app_handle.clone(),
        ));
    let resumer = Arc::new(AgentWorkspacePrFixReviewPublishCommandResumer {
        app_state: state.inner().clone(),
        execution_state,
    });
    build_agent_workspace_pr_supervision_recovery_deps(
        state.inner(),
        runtime_app_handle,
        Some(transition_service),
        Some(chat_service),
        Some(resumer),
    )
}

pub(crate) async fn is_exact_terminal_pr_autofix_completion(
    state: &AppState,
    payload: &AgentCompletionPayload,
) -> crate::error::AppResult<bool> {
    if payload.context_type != ChatContextType::Project {
        return Ok(false);
    }
    let Some(run_id) = payload
        .run_id
        .as_deref()
        .and_then(|value| value.parse::<AgentRunId>().ok())
    else {
        return Ok(false);
    };
    let conversation_id = ChatConversationId::from_string(payload.conversation_id.clone());
    let Some(run) = state.agent_run_repo.get_by_id(&run_id).await? else {
        return Ok(false);
    };
    if run.conversation_id != conversation_id || !run.status.is_terminal() {
        return Ok(false);
    }
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await?
    else {
        return Ok(false);
    };
    let Some(pr_number) = workspace.publication_pr_number else {
        return Ok(false);
    };
    if !is_exact_pr_autofix_run_for_pr(&run, pr_number) {
        return Ok(false);
    }
    let current_run = load_latest_exact_pr_autofix_run_for_pr(
        state.agent_run_repo.as_ref(),
        &conversation_id,
        pr_number,
    )
    .await?;
    Ok(current_run.is_some_and(|current_run| current_run.id == run_id))
}

pub(crate) fn spawn_auto_publish_from_completion_event<R>(
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

pub(crate) fn spawn_auto_publish_existing_pr<R>(
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
        match git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            auto_publish_existing_agent_workspace_pr_from_app_handle(
                &app_handle,
                conversation_id,
                trigger,
            )
            .await
        })
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

pub(crate) async fn auto_publish_stale_published_agent_workspace_prs_from_app_handle<R>(
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
    let workspaces = state
        .agent_conversation_workspace_repo
        .list_active_direct_published_workspaces()
        .await
        .map_err(|error| error.to_string())?;
    let mut published = 0;

    for workspace in workspaces {
        let conversation_id = workspace.conversation_id.clone();
        let attempt = state
            .agent_workspace_repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await;
        let blocked_exhausted = match attempt {
            Ok(Some(attempt)) => is_blocked_and_not_auto_retryable(&attempt),
            Ok(None) => false,
            Err(error) => {
                tracing::warn!(
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Skipping stale-base agent workspace auto-publish: durable repair gate failed closed"
                );
                continue;
            }
        };
        if blocked_exhausted {
            tracing::debug!(
                conversation_id = conversation_id.as_str(),
                "Skipping stale-base agent workspace auto-publish: durable repair is blocked and exhausted"
            );
            continue;
        }
        let Some(_guard) = begin_auto_publish(&conversation_id) else {
            continue;
        };

        match git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            auto_publish_existing_agent_workspace_pr(
                state.inner(),
                &execution_state,
                Some(app_handle.clone()),
                conversation_id,
                AutoPublishTrigger::BaseFreshness,
            )
            .await
        })
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

pub(crate) async fn auto_publish_existing_agent_workspace_pr_from_app_handle(
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
    auto_publish_existing_agent_workspace_pr(
        state.inner(),
        &execution_state,
        Some(app_handle.clone()),
        conversation_id,
        trigger,
    )
    .await
}

pub(crate) async fn auto_publish_existing_agent_workspace_pr<R>(
    state: &AppState,
    execution_state: &Arc<ExecutionState>,
    app_handle: Option<tauri::AppHandle<R>>,
    conversation_id: ChatConversationId,
    trigger: AutoPublishTrigger,
) -> Result<AutoPublishDecision, String>
where
    R: Runtime,
{
    let operation_scope = begin_publish_operation_scope(&conversation_id);
    let Some(mut workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::WorkspaceMissing,
        ));
    };

    if workspace.publication_push_status.as_deref()
        == Some(AGENT_WORKSPACE_PUBLISH_REDRIVE_PENDING_STATUS)
    {
        state
            .agent_conversation_workspace_repo
            .update_publication(
                &conversation_id,
                workspace.publication_pr_number,
                workspace.publication_pr_url.as_deref(),
                workspace.publication_pr_status.as_deref(),
                Some("refreshed"),
            )
            .await
            .map_err(|error| format!("publish re-drive admission failed: {error}"))?;
        workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "workspace disappeared during publish re-drive admission".to_string())?;
    }

    if workspace
        .publication_push_status
        .as_deref()
        .is_some_and(is_active_publish_status)
    {
        let now = chrono::Utc::now();
        let stale_cutoff = now
            - chrono::Duration::seconds(
                i64::try_from(
                    crate::infrastructure::agents::claude::git_runtime_config()
                        .agent_workspace_publish_lease_stale_secs,
                )
                .unwrap_or(i64::MAX),
            );
        let previous_owner_is_dead = match workspace.publish_lease_owner_run_id.as_deref() {
            Some(owner_run_id) if owner_run_id.starts_with("publish-operation:") => {
                !publish_operation_lease_is_live(
                    &conversation_id,
                    workspace.publish_lease_token.as_deref(),
                )
            }
            Some(owner_run_id) => match state
                .agent_run_repo
                .get_by_id(&AgentRunId::from_string(owner_run_id))
                .await
            {
                Ok(Some(run)) => !run.status.is_active(),
                Ok(None) => true,
                Err(error) => {
                    tracing::warn!(conversation_id = %conversation_id, error = %error, "Refused auto-publish lease reclaim after an unreadable owner run");
                    false
                }
            },
            None => workspace.updated_at <= stale_cutoff,
        };
        if !previous_owner_is_dead {
            return Ok(AutoPublishDecision::Skip(
                AutoPublishSkipReason::PublishAlreadyActive,
            ));
        }
        let admission_token = Uuid::new_v4().to_string();
        let claim = state
            .agent_conversation_workspace_repo
            .claim_publish_lease(
                &conversation_id,
                &format!("publish-operation:{conversation_id}"),
                &admission_token,
                now,
                workspace.publish_lease_token.as_deref(),
                true,
            )
            .await
            .map_err(|error| format!("publish lease reclaim failed closed: {error}"))?;
        if matches!(
            claim,
            crate::domain::repositories::AgentWorkspacePublishLeaseClaim::HeldByLiveOwner
        ) {
            return Ok(AutoPublishDecision::Skip(
                AutoPublishSkipReason::PublishAlreadyActive,
            ));
        }
        spawn_publish_operation_lease_heartbeat_for_scope(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation_id.clone(),
            admission_token.clone(),
            &operation_scope,
        );
        let release = state
            .agent_conversation_workspace_repo
            .release_publish_lease(
                &conversation_id,
                &admission_token,
                Some("refreshed"),
                chrono::Utc::now(),
            )
            .await;
        stop_publish_operation_lease_heartbeat(&conversation_id, &admission_token);
        let released =
            release.map_err(|error| format!("publish lease re-drive release failed: {error}"))?;
        if !released {
            return Err("publish lease re-drive release lost ownership".to_string());
        }
        workspace = state
            .agent_conversation_workspace_repo
            .get_by_conversation_id(&conversation_id)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "workspace disappeared after publish lease reclaim".to_string())?;
    }

    if let Some(reason) = static_auto_publish_skip_reason(&workspace) {
        return Ok(AutoPublishDecision::Skip(reason));
    }

    let repair_attempt = state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .map_err(|error| format!("durable repair gate failed closed: {error}"))?;
    if repair_attempt
        .as_ref()
        .is_some_and(is_blocked_and_not_auto_retryable)
    {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::DurableRepairBlockedExhausted,
        ));
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| format!("Project not found: {}", workspace.project_id))?;
    let publish_target =
        resolve_agent_workspace_publish_target(state, &project, &workspace).await?;
    if workspace.publication_pr_number.is_none()
        && publish_target.plan_branch.is_none()
        && (!project.github_pr_enabled
            || !matches!(
                inspect_repository_capability(&publish_target.worktree_path).await,
                RepositoryCapability::Github { .. }
            ))
    {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::NewPrUnavailable,
        ));
    }
    if publish_target.plan_branch.as_ref().is_some()
        && publish_target
            .plan_branch
            .as_ref()
            .and_then(|branch| branch.pr_number)
            .is_none()
    {
        return Ok(AutoPublishDecision::Skip(
            AutoPublishSkipReason::NoExistingPr,
        ));
    }
    let facts = collect_auto_publish_facts(&project, &workspace, &publish_target).await?;
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

pub(crate) async fn publish_was_routed_to_agent_repair(
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

pub(crate) async fn collect_auto_publish_facts(
    project: &Project,
    workspace: &AgentConversationWorkspace,
    publish_target: &AgentConversationWorkspacePublishTarget,
) -> Result<AutoPublishFacts, String> {
    let worktree_path = &publish_target.worktree_path;
    let has_uncommitted_changes = GitService::has_uncommitted_changes(worktree_path)
        .await
        .map_err(|error| error.to_string())?;
    let base_resolution = if workspace.mode == AgentConversationWorkspaceMode::Edit {
        Some(
            resolve_workspace_base(project, workspace)
                .await
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let base_is_blocked = base_resolution
        .as_ref()
        .is_some_and(|resolution| resolution.status == BaseStatus::Blocked);
    let effective_base_ref = if base_is_blocked {
        None
    } else {
        Some(if let Some(base_resolution) = base_resolution.as_ref() {
            base_resolution
                .effective_checkout_ref()
                .map_err(|error| error.to_string())?
                .to_string()
        } else {
            publish_target.base_ref.clone()
        })
    };
    let unpublished_commit_count = if let Some(effective_base_ref) = effective_base_ref.as_deref() {
        Some(
            count_publishable_commits_with_base_fallback(
                worktree_path,
                &publish_target.branch_name,
                effective_base_ref,
            )
            .await
            .map_err(|error| error.to_string())?,
        )
    } else {
        count_unpublished_publish_commits(worktree_path, &publish_target.branch_name)
            .await
            .map_err(|error| error.to_string())?
    };
    let base_is_ahead = if let Some(effective_base_ref) = effective_base_ref.as_deref() {
        inspect_publish_branch_freshness_for_source_after_fetch(
            worktree_path,
            effective_base_ref,
            &publish_target.branch_name,
            workspace.base_commit.as_deref(),
        )
        .await
        .map_err(|error| error.to_string())?
        .is_base_ahead
    } else {
        false
    };

    Ok(AutoPublishFacts {
        has_uncommitted_changes,
        unpublished_commit_count,
        base_is_ahead,
        base_is_blocked,
    })
}

pub(crate) fn should_auto_publish_existing_pr(
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

pub(crate) fn static_auto_publish_skip_reason(
    workspace: &AgentConversationWorkspace,
) -> Option<AutoPublishSkipReason> {
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Some(AutoPublishSkipReason::InactiveWorkspace);
    }
    let linked_ideation_plan_workspace = workspace.mode == AgentConversationWorkspaceMode::Ideation
        && workspace.linked_plan_branch_id.is_some();
    if workspace.mode != AgentConversationWorkspaceMode::Edit && !linked_ideation_plan_workspace {
        return Some(AutoPublishSkipReason::NotEditWorkspace);
    }
    if workspace.is_execution_owned() && !linked_ideation_plan_workspace {
        return Some(AutoPublishSkipReason::ExecutionOwnedWorkspace);
    }
    if workspace.publication_pr_number.is_none() {
        if linked_ideation_plan_workspace {
            if !workspace.auto_publish_enabled {
                return Some(AutoPublishSkipReason::AutoPublishDisabled);
            }
        } else if !workspace.auto_publish_initial_pr_enabled {
            return Some(AutoPublishSkipReason::InitialPrAutoPublishDisabled);
        }
    } else if !workspace.auto_publish_enabled {
        return Some(AutoPublishSkipReason::AutoPublishDisabled);
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

pub(super) fn is_active_publish_status(status: &str) -> bool {
    matches!(
        status,
        "checking" | "committing" | "refreshing" | "describing" | "pushing" | "needs_agent"
    )
}

fn auto_publish_in_flight() -> &'static DashMap<String, ()> {
    static AUTO_PUBLISH_IN_FLIGHT: OnceLock<DashMap<String, ()>> = OnceLock::new();
    AUTO_PUBLISH_IN_FLIGHT.get_or_init(DashMap::new)
}

pub(crate) fn begin_auto_publish(conversation_id: &ChatConversationId) -> Option<AutoPublishGuard> {
    let conversation_id = conversation_id.as_str().to_string();
    match auto_publish_in_flight().entry(conversation_id.clone()) {
        Entry::Occupied(_) => None,
        Entry::Vacant(entry) => {
            entry.insert(());
            Some(AutoPublishGuard { conversation_id })
        }
    }
}
