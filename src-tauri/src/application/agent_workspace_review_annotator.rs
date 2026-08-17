//! Background hunk annotator dispatched after a Workspace Review settles.
//!
//! Hunk annotations only feed the Commit & Publish walkthrough — they never affect the review
//! gate. Keeping them in the reviewer's run put that work in exactly the tail where the wrapper
//! deadline fires, so a finished review could be discarded over annotation work nobody was
//! blocking on. They now run as a separate, best-effort agent after settlement.
//!
//! Everything here is fail-soft by design: a dispatch failure logs and returns, and the annotator
//! holds no tool that can touch gate or outcome state.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::application::app_state::AppState;
use crate::application::chat_service::{
    ChatService, SendCallerContext, SendMessageOptions, SendQueuePolicy,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentRunId, AgentWorkspaceReviewHunkAnnotation, ChatContextType,
    ChatConversation,
};
use crate::domain::services::RunningAgentKey;
use crate::error::AppResult;
use crate::infrastructure::agents::claude::agent_names;

use super::agent_workspace_review::{AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewTarget};

const ANNOTATOR_LOG_TARGET: &str = "ralphx_lib::application::agent_workspace_review_annotator";

/// Carries unchanged files' annotations onto the review's new artifact version.
///
/// Annotation rows are keyed to `artifact_id`, and every review cycle writes a new artifact
/// version, so without this every cycle re-annotates the whole delta from zero.
///
/// The load-bearing property is that hunk anchors are **per-file**: `@@ -a,b +c,d @@` offsets are
/// relative to that file's own diff, so a file whose patch-vs-base is byte-identical between
/// cycles has byte-identical anchors and its annotations stay exactly valid. Line numbers do not
/// shift because some other file changed.
///
/// Correctness rests entirely on the hash, never on a head-delta: if the base moves, a file's
/// patch-vs-base can change while `prev_head..head` reports nothing, which would carry annotations
/// onto genuinely different hunks. Any file whose hash is missing on either side is annotated
/// fresh — a stale annotation describing code that has since changed is worse than none.
///
/// Returns the number of carried rows. Never fails the caller: this is an optimization.
pub(crate) async fn carry_forward_workspace_review_annotations(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> usize {
    match carry_forward_inner(state, workspace, target).await {
        Ok(carried) => carried,
        Err(error) => {
            warn!(
                target: ANNOTATOR_LOG_TARGET,
                operation = "annotation_carry_forward_failed",
                conversation_id = %workspace.conversation_id,
                error = %error,
                "Failed to carry annotations forward; the annotator will regenerate them"
            );
            0
        }
    }
}

async fn carry_forward_inner(
    state: &AppState,
    workspace: &AgentConversationWorkspace,
    target: &AgentWorkspaceReviewTarget,
) -> AppResult<usize> {
    let Some(monitor) = state
        .agent_conversation_workspace_repo
        .get_workspace_review_monitor(&workspace.conversation_id)
        .await?
    else {
        return Ok(0);
    };
    // Cycle 1 has no prior artifact version to carry from.
    let (Some(previous_artifact_id), Some(artifact_id), Some(artifact_version)) = (
        monitor.previous_version_id.clone(),
        monitor.review_artifact_id.clone(),
        monitor.review_artifact_version,
    ) else {
        return Ok(0);
    };

    let previous = state
        .agent_conversation_workspace_repo
        .list_workspace_review_hunk_annotations(&workspace.conversation_id, &previous_artifact_id)
        .await?;
    if previous.is_empty() {
        return Ok(0);
    }

    let selections = previous
        .iter()
        .filter(|annotation| annotation.file_patch_hash.is_some())
        .map(|annotation| (annotation.path.clone(), annotation.diff_source.clone()))
        .collect::<BTreeSet<_>>();
    if selections.is_empty() {
        return Ok(0);
    }
    let current_hashes =
        crate::application::agent_workspace_review_diff::workspace_review_file_patch_hashes(
            target,
            &selections,
        );

    let carried = previous
        .into_iter()
        .filter(|annotation| {
            let Some(previous_hash) = annotation.file_patch_hash.as_deref() else {
                return false;
            };
            current_hashes
                .get(&(annotation.path.clone(), annotation.diff_source.clone()))
                .is_some_and(|current_hash| current_hash == previous_hash)
        })
        .map(|annotation| AgentWorkspaceReviewHunkAnnotation {
            id: uuid::Uuid::new_v4().to_string(),
            artifact_id: artifact_id.clone(),
            artifact_version,
            head_sha: target.head_sha.clone(),
            diff_fingerprint: target.diff_fingerprint.clone(),
            ..annotation
        })
        .collect::<Vec<_>>();
    if carried.is_empty() {
        return Ok(0);
    }

    let carried_count = carried.len();
    state
        .agent_conversation_workspace_repo
        .replace_workspace_review_hunk_annotations(
            &workspace.conversation_id,
            &artifact_id,
            carried,
        )
        .await?;
    info!(
        target: ANNOTATOR_LOG_TARGET,
        operation = "annotation_carry_forward_applied",
        conversation_id = %workspace.conversation_id,
        carried_count,
        "Carried unchanged-file annotations onto the new Review artifact version"
    );
    Ok(carried_count)
}

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
    // Carry first: every hunk this restores is one the annotator no longer has to look at, since
    // `missing_workspace_review_hunk_anchors` reports carried hunks as covered.
    carry_forward_workspace_review_annotations(state, workspace, target).await;
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
        crate::infrastructure::agents::workspace_review_config().reviewer_idle_timeout_secs;
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

// --- Hunk-anchor / annotation key helpers -------------------------------------------------------
// These live in the application layer so the application tests can assert annotation coverage
// without importing upward into http_server.

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct WorkspaceReviewHunkAnnotationKey {
    pub path: String,
    pub source: String,
    pub hunk_header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
}

impl From<&AgentWorkspaceReviewHunkAnnotation> for WorkspaceReviewHunkAnnotationKey {
    fn from(value: &AgentWorkspaceReviewHunkAnnotation) -> Self {
        Self {
            path: value.path.clone(),
            source: value.diff_source.clone(),
            hunk_header: value.hunk_header.clone(),
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

impl From<&AgentWorkspaceReviewHunkAnchor> for WorkspaceReviewHunkAnnotationKey {
    fn from(value: &AgentWorkspaceReviewHunkAnchor) -> Self {
        Self {
            path: value.path.clone(),
            source: value.source.clone(),
            hunk_header: value.hunk_header.clone(),
            old_start: value.old_start,
            old_lines: value.old_lines,
            new_start: value.new_start,
            new_lines: value.new_lines,
        }
    }
}

/// Returns anchors in `target` that are not covered by any of the given annotations.
pub(crate) fn missing_workspace_review_hunk_anchors(
    target: &AgentWorkspaceReviewTarget,
    annotations: &[AgentWorkspaceReviewHunkAnnotation],
) -> Vec<AgentWorkspaceReviewHunkAnchor> {
    let covered = annotations
        .iter()
        .map(WorkspaceReviewHunkAnnotationKey::from)
        .collect::<BTreeSet<_>>();
    target
        .review_packet
        .hunk_anchors
        .iter()
        .filter(|anchor| !covered.contains(&WorkspaceReviewHunkAnnotationKey::from(*anchor)))
        .cloned()
        .collect()
}

/// Merges `updates` onto `existing`, with updates winning on key collision.
pub(crate) fn merge_workspace_review_hunk_annotations(
    existing: Vec<AgentWorkspaceReviewHunkAnnotation>,
    updates: Vec<AgentWorkspaceReviewHunkAnnotation>,
) -> Vec<AgentWorkspaceReviewHunkAnnotation> {
    let mut merged = BTreeMap::new();
    for annotation in existing {
        merged.insert(
            WorkspaceReviewHunkAnnotationKey::from(&annotation),
            annotation,
        );
    }
    for annotation in updates {
        merged.insert(
            WorkspaceReviewHunkAnnotationKey::from(&annotation),
            annotation,
        );
    }
    merged.into_values().collect()
}

/// Test seam: application tests can call this to assert annotation-coverage contracts without
/// reaching into the http_server layer.
#[doc(hidden)]
pub fn missing_workspace_review_hunk_anchors_for_test(
    target: &AgentWorkspaceReviewTarget,
    annotations: &[AgentWorkspaceReviewHunkAnnotation],
) -> Vec<AgentWorkspaceReviewHunkAnchor> {
    missing_workspace_review_hunk_anchors(target, annotations)
}
