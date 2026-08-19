use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tauri::{Emitter, Manager, Runtime};

use crate::application::agent_workspace_review::{
    load_agent_workspace_review_context, workspace_review_mode_is_eligible,
};
use crate::application::agent_workspace_review_auto_merge::{
    start_guarded_agent_workspace_review, WorkspaceReviewStartOrigin,
};
use crate::application::git_service::git_cmd;
use crate::application::AppState;
use crate::commands::ExecutionState;
use crate::domain::entities::{
    workspace_review_fixer_status_is_active, AgentConversationWorkspace,
    AgentConversationWorkspaceStatus, AgentRunId, AgentRunStatus, AgentWorkspaceReviewGateStatus,
    AgentWorkspaceReviewMonitor, ChatContextType, ChatConversationId,
};
use crate::domain::services::running_agent_registry::RunningAgentKey;

const AUTO_REVIEW_COMPLETION_SETTLE_DELAY: Duration = Duration::from_millis(250);

pub(crate) struct AutoReviewGuard {
    conversation_id: String,
}

impl Drop for AutoReviewGuard {
    fn drop(&mut self) {
        auto_review_in_flight().remove(&self.conversation_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoReviewDecision {
    Started,
    Skipped(AutoReviewSkipReason),
}

pub(crate) type WorkspaceChangedEmitter = Box<dyn Fn(&ChatConversationId) + Send + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoReviewStartAction {
    Start,
    Skip(AutoReviewSkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoReviewTrigger {
    AgentCompletion,
    BaseUpdate,
}

impl AutoReviewTrigger {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::AgentCompletion => "agent_completion",
            Self::BaseUpdate => "base_update",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoReviewSkipReason {
    WorkspaceMissing,
    InactiveWorkspace,
    NotReviewableMode,
    ManualOnlyArchived,
    ManualOnlyTerminalPr,
    NoReviewableChanges,
    GateNotRequired,
    WorkspaceAutomationOff,
    AlreadyReviewing,
    BlockingFindings,
    ReviewFailed,
    RelatedRuntimeGenerating,
    RepairAttemptActive,
    ReviewFixerActive,
    StartSkipped,
}

impl AutoReviewSkipReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceMissing => "workspace_missing",
            Self::InactiveWorkspace => "inactive_workspace",
            Self::NotReviewableMode => "not_reviewable_mode",
            Self::ManualOnlyArchived => "manual_only_archived",
            Self::ManualOnlyTerminalPr => "manual_only_terminal_pr",
            Self::NoReviewableChanges => "no_reviewable_changes",
            Self::GateNotRequired => "gate_not_required",
            Self::WorkspaceAutomationOff => "workspace_automation_off",
            Self::AlreadyReviewing => "already_reviewing",
            Self::BlockingFindings => "blocking_findings",
            Self::ReviewFailed => "review_failed",
            Self::RelatedRuntimeGenerating => "related_runtime_generating",
            Self::RepairAttemptActive => "repair_attempt_active",
            Self::ReviewFixerActive => "review_fixer_active",
            Self::StartSkipped => "start_skipped",
        }
    }
}

pub(crate) fn spawn_auto_review_from_completion_payload<R>(
    app_handle: tauri::AppHandle<R>,
    event_name: &'static str,
    completed_conversation_id: ChatConversationId,
) where
    R: Runtime,
{
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(AUTO_REVIEW_COMPLETION_SETTLE_DELAY).await;
        match resolve_workspace_conversation_id_for_review_event(
            &app_handle,
            &completed_conversation_id,
        )
        .await
        {
            Ok(Some(workspace_conversation_id)) => {
                spawn_auto_review_for_workspace(app_handle, event_name, workspace_conversation_id);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(
                    event_name,
                    conversation_id = completed_conversation_id.as_str(),
                    error = %error,
                    "Failed to resolve auto-review workspace conversation"
                );
            }
        }
    });
}

pub(crate) fn spawn_auto_review_for_workspace<R>(
    app_handle: tauri::AppHandle<R>,
    event_name: &'static str,
    conversation_id: ChatConversationId,
) where
    R: Runtime,
{
    let Some(_guard) = begin_auto_review(&conversation_id) else {
        tracing::debug!(
            event_name,
            trigger = AutoReviewTrigger::AgentCompletion.as_str(),
            conversation_id = conversation_id.as_str(),
            reason = "already_in_flight",
            "Skipped agent workspace auto-review"
        );
        return;
    };

    tauri::async_runtime::spawn(async move {
        let _guard = _guard;
        match git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            maybe_start_auto_review_from_app_handle(&app_handle, conversation_id.clone()).await
        })
        .await
        {
            Ok(AutoReviewDecision::Started) => {
                let _ = app_handle.emit(
                    "agent:workspace_changed",
                    serde_json::json!({ "conversation_id": conversation_id.as_str() }),
                );
            }
            Ok(AutoReviewDecision::Skipped(reason)) => {
                tracing::debug!(
                    event_name,
                    trigger = AutoReviewTrigger::AgentCompletion.as_str(),
                    conversation_id = conversation_id.as_str(),
                    reason = reason.as_str(),
                    "Skipped agent workspace auto-review"
                );
            }
            Err(error) => {
                tracing::warn!(
                    event_name,
                    trigger = AutoReviewTrigger::AgentCompletion.as_str(),
                    conversation_id = conversation_id.as_str(),
                    error = %error,
                    "Agent workspace auto-review failed"
                );
            }
        }
    });
}

pub(crate) async fn resolve_workspace_conversation_id_for_review_event<R>(
    app_handle: &tauri::AppHandle<R>,
    conversation_id: &ChatConversationId,
) -> Result<Option<ChatConversationId>, String>
where
    R: Runtime,
{
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    if state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Ok(Some(conversation_id.clone()));
    }

    let Some(conversation) = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };

    let Some(parent_id) = conversation.parent_conversation_id.as_deref() else {
        return Ok(None);
    };
    let parent_id = ChatConversationId::from_string(parent_id);
    if state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&parent_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some()
    {
        Ok(Some(parent_id))
    } else {
        Ok(None)
    }
}

pub(crate) async fn maybe_start_auto_review_from_app_handle<R>(
    app_handle: &tauri::AppHandle<R>,
    conversation_id: ChatConversationId,
) -> Result<AutoReviewDecision, String>
where
    R: Runtime,
{
    let state = app_handle
        .try_state::<AppState>()
        .ok_or_else(|| "AppState is not available".to_string())?;
    let execution_state = app_handle
        .try_state::<Arc<ExecutionState>>()
        .ok_or_else(|| "ExecutionState is not available".to_string())?
        .inner()
        .clone();
    let Some(workspace) = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .map_err(|error| error.to_string())?
    else {
        return Ok(AutoReviewDecision::Skipped(
            AutoReviewSkipReason::WorkspaceMissing,
        ));
    };

    maybe_start_auto_review(state.inner(), &execution_state, &workspace).await
}

pub(crate) fn spawn_auto_review_after_workspace_change(
    state: AppState,
    execution_state: Arc<ExecutionState>,
    workspace: AgentConversationWorkspace,
    trigger: AutoReviewTrigger,
    workspace_changed_emitter: Option<WorkspaceChangedEmitter>,
) -> bool {
    let conversation_id = workspace.conversation_id.clone();
    let Some(_guard) = begin_auto_review(&conversation_id) else {
        tracing::debug!(
            trigger = trigger.as_str(),
            conversation_id = conversation_id.as_str(),
            reason = "already_in_flight",
            "Skipped agent workspace auto-review"
        );
        return false;
    };

    tauri::async_runtime::spawn(async move {
        let _guard = _guard;
        let decision = git_cmd::with_git_command_lane(git_cmd::GitCommandLane::Background, async {
            maybe_start_auto_review(&state, execution_state.as_ref(), &workspace).await
        })
        .await;
        handle_auto_review_workspace_change_result(
            trigger,
            &conversation_id,
            decision,
            workspace_changed_emitter.as_ref(),
        );
    });
    true
}

pub(crate) fn handle_auto_review_workspace_change_result(
    trigger: AutoReviewTrigger,
    conversation_id: &ChatConversationId,
    decision: Result<AutoReviewDecision, String>,
    workspace_changed_emitter: Option<&WorkspaceChangedEmitter>,
) -> bool {
    match decision {
        Ok(AutoReviewDecision::Started) => {
            if let Some(emit_workspace_changed) = workspace_changed_emitter {
                emit_workspace_changed(conversation_id);
            }
            tracing::debug!(
                trigger = trigger.as_str(),
                conversation_id = conversation_id.as_str(),
                "Started agent workspace auto-review"
            );
            true
        }
        Ok(AutoReviewDecision::Skipped(reason)) => {
            tracing::debug!(
                trigger = trigger.as_str(),
                conversation_id = conversation_id.as_str(),
                reason = reason.as_str(),
                "Skipped agent workspace auto-review"
            );
            false
        }
        Err(error) => {
            tracing::warn!(
                trigger = trigger.as_str(),
                conversation_id = conversation_id.as_str(),
                error = %error,
                "Agent workspace auto-review failed"
            );
            false
        }
    }
}

pub(crate) async fn resolve_auto_review_start_action(
    state: &AppState,
    execution_state: &ExecutionState,
    workspace: &AgentConversationWorkspace,
) -> Result<AutoReviewStartAction, String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| workspace.clone());
    let workspace = &workspace;
    if workspace.status == AgentConversationWorkspaceStatus::Archived {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::ManualOnlyArchived,
        ));
    }
    if workspace.status != AgentConversationWorkspaceStatus::Active {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::InactiveWorkspace,
        ));
    }
    if !workspace_review_mode_is_eligible(workspace.mode) {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::NotReviewableMode,
        ));
    }
    if workspace.has_terminal_publication_pr_status() {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::ManualOnlyTerminalPr,
        ));
    }
    let review_settings = state
        .review_settings_repo
        .get_settings()
        .await
        .map_err(|error| error.to_string())?;
    if !review_settings
        .effective_workspace_review_automation(workspace.review_automation_override)
        .auto_review
    {
        return Ok(AutoReviewStartAction::Skip(
            if workspace.review_automation_override == Some(false)
                && review_settings.require_workspace_review
            {
                AutoReviewSkipReason::WorkspaceAutomationOff
            } else {
                AutoReviewSkipReason::GateNotRequired
            },
        ));
    }

    let context = load_agent_workspace_review_context(state, workspace)
        .await
        .map_err(|error| error.to_string())?;
    if context.target.is_none() {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::NoReviewableChanges,
        ));
    }

    match context.monitor.review_gate_status {
        AgentWorkspaceReviewGateStatus::Required => {}
        AgentWorkspaceReviewGateStatus::Reviewing => {
            return Ok(AutoReviewStartAction::Skip(
                AutoReviewSkipReason::AlreadyReviewing,
            ));
        }
        AgentWorkspaceReviewGateStatus::Blocking => {
            return Ok(AutoReviewStartAction::Skip(
                AutoReviewSkipReason::BlockingFindings,
            ));
        }
        AgentWorkspaceReviewGateStatus::Failed => {
            return Ok(AutoReviewStartAction::Skip(
                AutoReviewSkipReason::ReviewFailed,
            ));
        }
        AgentWorkspaceReviewGateStatus::NotRequired | AgentWorkspaceReviewGateStatus::Passed => {
            return Ok(AutoReviewStartAction::Skip(
                AutoReviewSkipReason::GateNotRequired,
            ));
        }
    }

    // Reviewing a head that a repair or fixer is actively rewriting wastes the run and inflates
    // the convergence counter. Defer instead: the existing repair-boundary starter and PR-fix
    // completion handoff already re-dispatch once the head settles, so no new machinery is needed.
    if workspace_review_fixer_status_is_active(context.monitor.review_fixer_status.as_deref()) {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::ReviewFixerActive,
        ));
    }
    // Fail closed toward deferral: a repair-repo read error must not read as "no repair active".
    // Deferring is always recoverable through those same handoffs; reviewing a doomed head is not.
    let repair_attempt_active = match state
        .agent_workspace_repair_repo
        .get_current_repair_attempt(&workspace.conversation_id)
        .await
    {
        Ok(attempt) => attempt.is_some(),
        Err(error) => {
            tracing::warn!(
                target: "ralphx_lib::commands::agent_workspace_auto_review",
                operation = "auto_review_repair_lookup_failed",
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Could not read repair state; deferring auto-review"
            );
            true
        }
    };
    if repair_attempt_active {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::RepairAttemptActive,
        ));
    }

    if related_workspace_runtime_is_generating(state, execution_state, workspace, &context.monitor)
        .await?
    {
        return Ok(AutoReviewStartAction::Skip(
            AutoReviewSkipReason::RelatedRuntimeGenerating,
        ));
    }

    Ok(AutoReviewStartAction::Start)
}

pub(crate) async fn maybe_start_auto_review(
    state: &AppState,
    execution_state: &ExecutionState,
    workspace: &AgentConversationWorkspace,
) -> Result<AutoReviewDecision, String> {
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(&workspace.conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .unwrap_or_else(|| workspace.clone());
    match resolve_auto_review_start_action(state, execution_state, &workspace).await? {
        AutoReviewStartAction::Start => {}
        AutoReviewStartAction::Skip(reason) => return Ok(AutoReviewDecision::Skipped(reason)),
    }

    let started = start_guarded_agent_workspace_review(
        Arc::new(state.clone()),
        &workspace,
        false,
        WorkspaceReviewStartOrigin::Automated,
        None,
    )
    .await
    .map_err(|error| error.to_string())?;
    if started.started {
        Ok(AutoReviewDecision::Started)
    } else {
        Ok(AutoReviewDecision::Skipped(
            AutoReviewSkipReason::StartSkipped,
        ))
    }
}

pub(crate) async fn related_workspace_runtime_is_generating(
    state: &AppState,
    execution_state: &ExecutionState,
    workspace: &AgentConversationWorkspace,
    monitor: &AgentWorkspaceReviewMonitor,
) -> Result<bool, String> {
    let mut conversation_ids = HashSet::new();
    conversation_ids.insert(workspace.conversation_id.clone());
    if let Some(review_conversation_id) = monitor.review_conversation_id.as_ref() {
        conversation_ids.insert(review_conversation_id.clone());
    }

    let conversations = state
        .chat_conversation_repo
        .get_by_context(ChatContextType::Project, workspace.project_id.as_str())
        .await
        .map_err(|error| error.to_string())?;
    let parent_id = workspace.conversation_id.as_str();
    for conversation in conversations {
        if conversation.parent_conversation_id.as_deref() == Some(parent_id.as_str()) {
            conversation_ids.insert(conversation.id);
        }
    }

    for conversation_id in conversation_ids {
        if conversation_is_generating(state, execution_state, &conversation_id).await? {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn conversation_is_generating(
    state: &AppState,
    execution_state: &ExecutionState,
    conversation_id: &ChatConversationId,
) -> Result<bool, String> {
    let context_id = conversation_id.as_str();
    let key = RunningAgentKey::new(ChatContextType::Project.to_string(), &context_id);
    if let Some(info) = state.running_agent_registry.get(&key).await {
        if execution_state.is_interactive_idle(&interactive_slot_key(&context_id)) {
            return Ok(false);
        }
        if info.agent_run_id.is_empty() {
            return Ok(true);
        }
        let run = state
            .agent_run_repo
            .get_by_id(&AgentRunId::from_string(info.agent_run_id))
            .await
            .map_err(|error| error.to_string())?;
        return Ok(run
            .map(|run| run.status == AgentRunStatus::Running)
            .unwrap_or(true));
    }

    Ok(state
        .agent_run_repo
        .get_active_for_conversation(conversation_id)
        .await
        .map_err(|error| error.to_string())?
        .is_some())
}

pub(crate) fn interactive_slot_key(context_id: &str) -> String {
    format!("{}/{}", ChatContextType::Project, context_id)
}

pub(crate) fn begin_auto_review(conversation_id: &ChatConversationId) -> Option<AutoReviewGuard> {
    let conversation_id = conversation_id.as_str().to_string();
    match auto_review_in_flight().entry(conversation_id.clone()) {
        Entry::Occupied(_) => None,
        Entry::Vacant(entry) => {
            entry.insert(());
            Some(AutoReviewGuard { conversation_id })
        }
    }
}

fn auto_review_in_flight() -> &'static DashMap<String, ()> {
    static IN_FLIGHT: OnceLock<DashMap<String, ()>> = OnceLock::new();
    IN_FLIGHT.get_or_init(DashMap::new)
}
