use crate::domain::entities::{
    merge_progress_event::{MergePhase, MergePhaseStatus},
    InternalStatus,
};
use crate::infrastructure::agents::claude::defer_merge_enabled;

/// Check if a main-branch merge should be deferred.
///
/// Returns `true` if the merge was deferred (caller should return early).
/// Defers when target is the base branch AND either:
/// 1. Sibling plan tasks are not all terminal
/// 2. Agents are still running (running_agent_count > 0)
pub(crate) async fn check_main_merge_deferral(
    tc: super::TaskCore<'_>,
    bp: super::BranchPair<'_>,
    base_branch: &str,
    running_agent_count: Option<u32>,
    event_sink: Option<&dyn ralphx_events::EventSink>,
) -> bool {
    let task = tc.task;
    let task_id_str = tc.task_id_str;
    let task_repo = tc.task_repo;
    let (source_branch, target_branch) = (bp.source_branch, bp.target_branch);
    if target_branch != base_branch || !defer_merge_enabled() {
        return false;
    }

    if let Some(ref session_id) = task.ideation_session_id {
        let siblings = task_repo
            .get_by_ideation_session(session_id)
            .await
            .unwrap_or_default();
        let all_siblings_terminal = siblings.iter().all(|t| {
            t.id == task.id || t.internal_status == InternalStatus::PendingMerge || t.is_terminal()
        });
        if !all_siblings_terminal {
            tracing::info!(
                task_id = task_id_str,
                session_id = %session_id,
                "Deferring main-branch merge: sibling plan tasks not yet terminal"
            );

            super::merge_helpers::set_main_merge_deferred_metadata(task);
            task.touch();

            if let Err(e) = task_repo.update(task).await {
                tracing::error!(error = %e, "Failed to set main_merge_deferred metadata");
                return true;
            }

            super::emit_merge_progress(
                event_sink,
                task_id_str,
                MergePhase::programmatic_merge(),
                MergePhaseStatus::Started,
                format!(
                    "Deferred merge to {} — waiting for sibling tasks to complete",
                    target_branch,
                ),
            );

            return true;
        }
    }

    if let Some(count) = running_agent_count {
        if count > 0 {
            tracing::info!(
                task_id = task_id_str,
                source_branch = %source_branch,
                target_branch = %target_branch,
                running_count = count,
                "Deferring main-branch merge: {} agents still running — \
                 merge will be retried when all agents complete",
                count
            );

            super::merge_helpers::set_main_merge_deferred_metadata(task);
            task.touch();

            if let Err(e) = task_repo.update(task).await {
                tracing::error!(error = %e, "Failed to set main_merge_deferred metadata");
                return true;
            }

            super::emit_merge_progress(
                event_sink,
                task_id_str,
                MergePhase::programmatic_merge(),
                MergePhaseStatus::Started,
                format!(
                    "Deferred merge to {} — waiting for {} agent(s) to complete",
                    target_branch, count
                ),
            );

            return true;
        }
    }

    tracing::debug!(
        task_id = task_id_str,
        running_count = running_agent_count.unwrap_or(0),
        proceeding = true,
        "check_main_merge_deferral: all guards passed — proceeding with merge"
    );
    false
}
