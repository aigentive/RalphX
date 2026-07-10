use std::path::PathBuf;

use crate::domain::entities::merge_progress_event::{MergePhase, MergePhaseStatus};
use crate::infrastructure::agents::claude::git_runtime_config;

use super::super::{cleanup_helpers, emit_merge_progress, TransitionHandler};

pub(super) async fn cancel_validation_and_stop_agents<'a>(
    handler: &TransitionHandler<'a>,
    task_id_str: &str,
    task: &crate::domain::entities::Task,
    event_sink: Option<&dyn ralphx_events::EventSink>,
) {
    // --- Step 0a: Cancel in-flight validation for this task ---
    if let Some((_, token)) = handler
        .machine
        .context
        .services
        .validation_tokens
        .remove(task_id_str)
    {
        token.cancel();
        tracing::info!(
            task_id = task_id_str,
            "pre_merge_cleanup: cancelled in-flight validation"
        );
    }

    // --- Step 0b: Stop running agents (SIGKILL immediate for merge cleanup) ---
    let step_start = std::time::Instant::now();
    emit_merge_progress(
        event_sink,
        task_id_str,
        MergePhase::new(MergePhase::MERGE_CLEANUP),
        MergePhaseStatus::Started,
        "Stopping running agents...".to_string(),
    );
    let agent_stop_timeout_secs = git_runtime_config().agent_stop_timeout_secs;
    let mut any_agent_was_running = false;
    for ctx_type in [
        crate::domain::entities::ChatContextType::Review,
        crate::domain::entities::ChatContextType::Merge,
    ] {
        // Defense-in-depth: if this is the Review agent context and the task has already
        // transitioned past Reviewing (e.g., to PendingMerge), skip stop_agent. The review
        // agent's job is done; stopping it here would kill the TCP connection that owns the
        // complete_review HTTP handler and cancel the entire inline merge pipeline.
        // This guard fires even if early-unregister in the complete_review handler missed
        // a timing edge (e.g., a different transition path).
        if ctx_type == crate::domain::entities::ChatContextType::Review
            && task.internal_status != crate::domain::entities::InternalStatus::Reviewing
        {
            tracing::info!(
                task_id = task_id_str,
                context_type = ?ctx_type,
                task_status = ?task.internal_status,
                "pre_merge_cleanup: skipping stop_agent for Review context — task already past Reviewing (self-sabotage guard)"
            );
            continue;
        }

        let stop_result = tokio::time::timeout(
            std::time::Duration::from_secs(agent_stop_timeout_secs),
            handler
                .machine
                .context
                .services
                .chat_service
                .stop_agent(ctx_type, task_id_str),
        )
        .await;
        match stop_result {
            Ok(Ok(true)) => {
                any_agent_was_running = true;
                tracing::info!(
                    task_id = task_id_str,
                    context_type = ?ctx_type,
                    "Stopped running agent before merge cleanup"
                );
            }
            Ok(Ok(false)) => {}
            Ok(Err(e)) => {
                any_agent_was_running = true;
                tracing::warn!(
                    task_id = task_id_str,
                    context_type = ?ctx_type,
                    error = %e,
                    "Failed to stop agent (non-fatal)"
                );
            }
            Err(_elapsed) => {
                any_agent_was_running = true;
                tracing::warn!(
                    task_id = task_id_str,
                    context_type = ?ctx_type,
                    timeout_secs = agent_stop_timeout_secs,
                    "stop_agent timed out (non-fatal)"
                );
            }
        }
    }
    // Scan for OS-level processes still holding worktree files open — only if agents were running
    if any_agent_was_running {
        emit_merge_progress(
            event_sink,
            task_id_str,
            MergePhase::new(MergePhase::MERGE_CLEANUP),
            MergePhaseStatus::Started,
            "Scanning worktree for orphaned processes...".to_string(),
        );
        if let Some(ref worktree_path) = task.worktree_path {
            let worktree_path_buf = PathBuf::from(worktree_path);
            if worktree_path_buf.exists() {
                let lsof_timeout = git_runtime_config().worktree_lsof_timeout_secs;
                let step_0b_timeout_secs = git_runtime_config().step_0b_kill_timeout_secs;
                match cleanup_helpers::os_thread_timeout(
                    std::time::Duration::from_secs(step_0b_timeout_secs),
                    crate::domain::services::kill_worktree_processes_async(
                        &worktree_path_buf,
                        lsof_timeout,
                        true, // merge cleanup: SIGKILL immediately
                    ),
                )
                .await
                {
                    Ok(()) => {}
                    Err(_os_elapsed) => {
                        tracing::warn!(
                            task_id = %task_id_str,
                            worktree = %worktree_path,
                            step_0b_timeout_secs,
                            "pre_merge_cleanup step 0b: kill_worktree_processes_async timed out — proceeding"
                        );
                    }
                }
            }
        }
        // Conditional settle sleep — only when agents were actually killed
        let agent_kill_settle_secs = git_runtime_config().agent_kill_settle_secs;
        if agent_kill_settle_secs > 0 {
            let settle_secs = agent_kill_settle_secs;
            tracing::info!(
                task_id = task_id_str,
                settle_secs,
                "pre_merge_cleanup: agents were killed, waiting {}s for process tree cleanup",
                settle_secs,
            );
            // Always use os_thread_timeout — immune to tokio timer-driver starvation.
            // One dormant OS thread per merge (settle_secs + 1s grace) is acceptable.
            match cleanup_helpers::os_thread_timeout(
                std::time::Duration::from_secs(settle_secs + 1),
                tokio::time::sleep(std::time::Duration::from_secs(settle_secs)),
            )
            .await
            {
                Ok(_) => {}
                Err(_elapsed) => {
                    tracing::warn!(
                        task_id = %task_id_str,
                        settle_secs,
                        "settle sleep watchdog fired — possible tokio timer starvation"
                    );
                }
            }
        }
    } else {
        tracing::info!(
            task_id = task_id_str,
            "pre_merge_cleanup: no agents running — skipping process scan and settle sleep"
        );
    }
    tracing::info!(
        task_id = task_id_str,
        elapsed_ms = step_start.elapsed().as_millis() as u64,
        "pre_merge_cleanup: step 0b complete"
    );
}
