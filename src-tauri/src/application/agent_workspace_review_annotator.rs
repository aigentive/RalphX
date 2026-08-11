//! Background hunk annotator dispatched after a Workspace Review settles.
//!
//! Hunk annotations only feed the Commit & Publish walkthrough — they never affect the review
//! gate. Keeping them in the reviewer's run put that work in exactly the tail where the wrapper
//! deadline fires, so a finished review could be discarded over annotation work nobody was
//! blocking on. They now run as a separate, best-effort agent after settlement.
//!
//! Everything here is fail-soft by design: a dispatch failure logs and returns, and the annotator
//! holds no tool that can touch gate or outcome state.

use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::application::app_state::AppState;
use crate::application::chat_service::{
    ChatService, SendCallerContext, SendMessageOptions, SendQueuePolicy,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentRunId, ChatContextType, ChatConversation,
};
use crate::domain::services::RunningAgentKey;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::agent_names;

use super::agent_workspace_review::AgentWorkspaceReviewTarget;

const ANNOTATOR_LOG_TARGET: &str = "ralphx_lib::application::agent_workspace_review_annotator";

/// Dispatches the annotator for a settled review, best effort.
///
/// Registers the run in `annotation_run_id` *before* launching, so the annotator's first write
/// cannot race ahead of its own authority. A failed launch leaves a run id that can never write,
/// which the next target refresh clears.
///
/// Never returns an error: annotation is a reading aid, and no failure here may change a settled
/// gate.
pub(crate) async fn dispatch_workspace_review_annotator(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) {
    let chat_service = state.build_chat_service();
    if let Err(error) =
        dispatch_with_chat_service(state, workspace, target, &chat_service).await
    {
        warn!(
            target: ANNOTATOR_LOG_TARGET,
            operation = "annotator_dispatch_failed",
            conversation_id = %workspace.conversation_id,
            project_id = %workspace.project_id,
            error = %error,
            "Failed to dispatch workspace Review hunk annotator; review gate is unaffected"
        );
    }
}

pub(crate) async fn dispatch_with_chat_service<S: ChatService + ?Sized>(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
    chat_service: &S,
) -> AppResult<()> {
    let runtime = state
        .resolve_workspace_role_runtime_for_project(
            workspace.project_id.as_str(),
            crate::domain::agents::RoutingRole::WorkspaceReviewer,
            agent_names::AGENT_WORKSPACE_ANNOTATOR,
            "workspace annotator provider",
        )
        .await?;

    let mut conversation = ChatConversation::new_project(workspace.project_id.clone());
    conversation.parent_conversation_id = Some(workspace.conversation_id.as_str());
    conversation.title = Some("Annotate reviewed changes".to_string());
    let annotator_conversation_id = state.chat_conversation_repo.create(conversation).await?.id;

    let annotator_run_id = AgentRunId::new();
    let annotator_run_id_value = annotator_run_id.to_string();

    // Reserve write authority before launch. The monitor is reloaded here rather than passed in so
    // this cannot resurrect a monitor snapshot taken before settlement persisted.
    let mut monitor = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?
        .ok_or_else(|| {
            crate::error::AppError::NotFound(
                "workspace Review monitor disappeared before annotator dispatch".to_string(),
            )
        })?;
    monitor.annotation_run_id = Some(annotator_run_id_value.clone());
    state
        .agent_conversation_workspace_repo
        .upsert_workspace_review_monitor(monitor)
        .await?;

    let send_result = chat_service
        .send_message(
            ChatContextType::Project,
            workspace.project_id.as_str(),
            &build_annotator_request_message(target),
            SendMessageOptions {
                preallocated_agent_run_id: Some(annotator_run_id),
                queue_policy: SendQueuePolicy::RequireImmediateStart,
                conversation_id_override: Some(annotator_conversation_id.clone()),
                runtime_source_override: Some(runtime.runtime_source),
                harness_override: runtime.harness,
                agent_name_override: Some(agent_names::AGENT_WORKSPACE_ANNOTATOR.to_string()),
                model_override: runtime.model,
                working_directory_override: Some(target.working_directory.clone()),
                logical_effort_override: runtime.logical_effort,
                approval_policy_override: runtime.approval_policy,
                sandbox_mode_override: runtime.sandbox_mode,
                service_tier_override: runtime.service_tier,
                force_new_provider_session: true,
                metadata: Some(annotator_request_metadata()),
                caller_context: SendCallerContext::UserInitiated,
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            crate::error::AppError::Infrastructure(format!(
                "failed to start workspace annotator chat: {error}"
            ))
        })?;

    info!(
        target: ANNOTATOR_LOG_TARGET,
        operation = "annotator_started",
        conversation_id = %workspace.conversation_id,
        annotator_conversation_id = %send_result.conversation_id,
        project_id = %workspace.project_id,
        run_id = %send_result.agent_run_id,
        target_scope = %target.scope,
        "Started workspace Review hunk annotator"
    );

    spawn_annotator_deadline(
        Arc::clone(&state.running_agent_registry),
        annotator_conversation_id.as_str().to_string(),
        send_result.agent_run_id.clone(),
    );
    Ok(())
}

/// Bounds the annotator run.
///
/// On expiry this stops the process and nothing else — no monitor write, no gate change, no
/// error surface. `stop_if_owned` keys on the run id, so a later run in the same conversation can
/// never be killed by an earlier deadline.
fn spawn_annotator_deadline(
    running_agent_registry: Arc<dyn crate::domain::services::RunningAgentRegistry>,
    annotator_conversation_id: String,
    annotator_run_id: String,
) {
    let timeout_secs =
        crate::infrastructure::agents::workspace_review_config().annotator_timeout_secs;
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(timeout_secs)).await;
        let key = RunningAgentKey::new("project", annotator_conversation_id.clone());
        match running_agent_registry
            .stop_if_owned(&key, &annotator_run_id)
            .await
        {
            Ok(Some(_)) => {
                info!(
                    target: ANNOTATOR_LOG_TARGET,
                    operation = "annotator_deadline_stopped",
                    annotator_conversation_id = %annotator_conversation_id,
                    run_id = %annotator_run_id,
                    timeout_secs,
                    "Stopped workspace Review annotator at its deadline"
                );
            }
            Ok(None) => {}
            Err(error) => {
                warn!(
                    target: ANNOTATOR_LOG_TARGET,
                    operation = "annotator_deadline_stop_failed",
                    annotator_conversation_id = %annotator_conversation_id,
                    run_id = %annotator_run_id,
                    error = %error,
                    "Failed to stop workspace Review annotator at its deadline"
                );
            }
        }
    });
}

fn annotator_request_metadata() -> String {
    serde_json::json!({
        "hidden_from_ui": true,
        "source": "workspace_review_annotator_request",
    })
    .to_string()
}

fn build_annotator_request_message(target: &AgentWorkspaceReviewTarget) -> String {
    format!(
        "The Workspace Review for the `{}` target has settled. Annotate its changed hunks for the \
         Commit & Publish walkthrough.\n\n\
         Call `get_workspace_review_context` for the target and its packet, then write short \
         per-hunk descriptions with `write_workspace_review_hunk_annotations`. Hunks already \
         covered by annotations carried forward from a previous cycle need no work. Skip \
         low-signal files. Partial coverage is fine; there is no completion call and the review \
         gate is already settled.",
        target.scope
    )
}
