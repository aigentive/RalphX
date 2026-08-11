use super::super::{is_first_clean_attempt, TransitionHandler, MERGE_DEBRIS_METADATA_KEYS};

pub(super) async fn maybe_skip_first_attempt_cleanup<'a>(
    handler: &TransitionHandler<'a>,
    task_id_str: &str,
    task: &crate::domain::entities::Task,
    cleanup_start: std::time::Instant,
) -> bool {
    // --- Phase 1 GUARD: first-attempt skip optimization (ROOT CAUSE #3) ---
    // If this is the first merge attempt AND no agents are running for this task,
    // skip all cleanup steps — there's no debris to clean.
    let is_first = is_first_clean_attempt(task);
    if is_first {
        // Quick agent check: are review/merge agents currently running?
        let review_running = handler
            .machine
            .context
            .services
            .chat_service
            .is_agent_running(
                crate::domain::entities::ChatContextType::Review,
                task_id_str,
            )
            .await;
        let merge_running = handler
            .machine
            .context
            .services
            .chat_service
            .is_agent_running(crate::domain::entities::ChatContextType::Merge, task_id_str)
            .await;

        if !review_running && !merge_running {
            tracing::info!(
                task_id = task_id_str,
                elapsed_us = cleanup_start.elapsed().as_micros() as u64,
                "pre_merge_cleanup: first clean attempt, no agents running — running cleanup to clear stale worktrees from prior crashes"
            );
            // Always proceed with cleanup — stale worktrees from prior crashed merges
            // must be cleaned even on the first attempt to prevent merge failures.
        }
        tracing::info!(
            task_id = task_id_str,
            review_running,
            merge_running,
            "pre_merge_cleanup: first attempt but agents running — proceeding with cleanup"
        );
    } else {
        let pipeline_active = task.merge_pipeline_active.is_some();
        let has_debris_metadata = task.metadata.as_ref().map_or(false, |s| {
            serde_json::from_str::<serde_json::Value>(s)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .map_or(true, |obj| {
                    MERGE_DEBRIS_METADATA_KEYS
                        .iter()
                        .any(|key| obj.contains_key(*key))
                })
        });
        let disk_exists = task
            .worktree_path
            .as_ref()
            .map_or(false, |p| std::path::Path::new(p).exists());
        tracing::info!(
            task_id = task_id_str,
            pipeline_active,
            has_debris_metadata,
            disk_exists,
            "pre_merge_cleanup: retry attempt (debris detected — pipeline active flag, metadata, or stale worktree on disk) — running full cleanup"
        );
    }

    false
}
