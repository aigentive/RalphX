// Helper functions for task_commands module

use crate::application::AppState;
use crate::application::chat_service::uses_execution_slot;
use crate::domain::entities::{ChatContextType, IdeationSessionId, InternalStatus, ProjectId, TaskId};
use tauri::{Emitter, State};

/// Default target for inject_task command
pub fn default_target() -> String {
    "backlog".to_string()
}

/// Emit execution:queue_changed event with current Ready task count.
///
/// This is called when a task moves to or from Ready status, providing real-time
/// queue count updates to the frontend's ExecutionControlBar.
///
/// This function is public so it can be reused by other command modules that
/// transition tasks to/from Ready status (e.g., review_commands::approve_fix_task).
pub async fn emit_queue_changed(
    state: &State<'_, AppState>,
    project_id: &ProjectId,
    app: &tauri::AppHandle,
) {
    // Count tasks currently in Ready status
    let queued_count = match state
        .task_repo
        .get_by_status(project_id, InternalStatus::Ready)
        .await
    {
        Ok(tasks) => tasks.len(),
        Err(e) => {
            tracing::warn!("Failed to count Ready tasks for queue_changed event: {}", e);
            return;
        }
    };

    let queued_message_count = match count_slot_consuming_queued_messages_for_project(
        state.inner(),
        project_id,
    )
    .await
    {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(
                "Failed to count queued agent messages for queue_changed event: {}",
                e
            );
            0
        }
    };

    // Phase 82: Include projectId in queue_changed event for per-project scoping
    let _ = app.emit(
        "execution:queue_changed",
        serde_json::json!({
            "queuedCount": queued_count,
            "queuedMessageCount": queued_message_count,
            "projectId": project_id.as_str(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }),
    );

    tracing::debug!(
        queued_count,
        queued_message_count,
        "Emitted execution:queue_changed event"
    );
}

pub async fn count_slot_consuming_queued_messages_for_project(
    app_state: &AppState,
    project_id: &ProjectId,
) -> Result<u32, String> {
    let mut count = 0u32;
    for key in app_state.message_queue.list_keys() {
        if !uses_execution_slot(key.context_type) {
            continue;
        }

        let matches_project = match key.context_type {
            ChatContextType::Ideation => {
                let session_id = IdeationSessionId::from_string(key.context_id.clone());
                match app_state
                    .ideation_session_repo
                    .get_by_id(&session_id)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    Some(session) => session.project_id == *project_id,
                    None => false,
                }
            }
            ChatContextType::TaskExecution | ChatContextType::Review | ChatContextType::Merge => {
                let task_id = TaskId::from_string(key.context_id.clone());
                match app_state
                    .task_repo
                    .get_by_id(&task_id)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    Some(task) => task.project_id == *project_id,
                    None => false,
                }
            }
            _ => false,
        };

        if !matches_project {
            continue;
        }

        count += app_state.message_queue.get_queued_with_key(&key).len() as u32;
    }

    Ok(count)
}

/// Emit a task lifecycle event (archived, restored, deleted).
///
/// These events share a common payload structure with task and project IDs.
pub fn emit_task_lifecycle_event(
    app: &tauri::AppHandle,
    event_name: &str,
    task_id: &str,
    project_id: &str,
) {
    let _ = app.emit(
        event_name,
        serde_json::json!({
            "taskId": task_id,
            "projectId": project_id,
        }),
    );
}

/// Maps an InternalStatus to a user-friendly label for the status dropdown
pub fn status_to_label(status: InternalStatus) -> String {
    match status {
        InternalStatus::Backlog => "Move to Backlog".to_string(),
        InternalStatus::Ready => "Ready for Work".to_string(),
        InternalStatus::Blocked => "Mark as Blocked".to_string(),
        InternalStatus::Executing => "Start Execution".to_string(),
        InternalStatus::QaRefining => "QA Refining".to_string(),
        InternalStatus::QaTesting => "QA Testing".to_string(),
        InternalStatus::QaPassed => "QA Passed".to_string(),
        InternalStatus::QaFailed => "QA Failed".to_string(),
        InternalStatus::PendingReview => "Send to Review".to_string(),
        InternalStatus::Reviewing => "AI Reviewing".to_string(),
        InternalStatus::ReviewPassed => "Review Passed".to_string(),
        InternalStatus::Escalated => "Escalated".to_string(),
        InternalStatus::RevisionNeeded => "Needs Revision".to_string(),
        InternalStatus::ReExecuting => "Re-executing".to_string(),
        InternalStatus::Approved => "Approve".to_string(),
        InternalStatus::PendingMerge => "Merging...".to_string(),
        InternalStatus::Merging => "Resolving Conflicts".to_string(),
        InternalStatus::WaitingOnPr => "Waiting on Pull Request".to_string(),
        InternalStatus::MergeIncomplete => "Merge Incomplete".to_string(),
        InternalStatus::MergeConflict => "Merge Conflict".to_string(),
        InternalStatus::Merged => "Merged".to_string(),
        InternalStatus::Failed => "Mark as Failed".to_string(),
        InternalStatus::Cancelled => "Cancel".to_string(),
        InternalStatus::Paused => "Paused".to_string(),
        InternalStatus::Stopped => "Stopped".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::count_slot_consuming_queued_messages_for_project;
    use crate::application::AppState;
    use crate::domain::entities::{ChatContextType, IdeationSession, InternalStatus, Project, Task};

    #[tokio::test]
    async fn test_count_slot_consuming_queued_messages_for_project_counts_all_slot_contexts() {
        let app_state = AppState::new_test();
        let project = Project::new("Queue Count Project".to_string(), "/test/queue-count".to_string());
        app_state.project_repo.create(project.clone()).await.unwrap();

        let review_task = app_state
            .task_repo
            .create(Task {
                internal_status: InternalStatus::Reviewing,
                ..Task::new(project.id.clone(), "Review queued".to_string())
            })
            .await
            .unwrap();
        let merge_task = app_state
            .task_repo
            .create(Task {
                internal_status: InternalStatus::Merging,
                ..Task::new(project.id.clone(), "Merge queued".to_string())
            })
            .await
            .unwrap();
        let session = app_state
            .ideation_session_repo
            .create(IdeationSession::new(project.id.clone()))
            .await
            .unwrap();

        app_state.message_queue.queue(
            ChatContextType::Review,
            review_task.id.as_str(),
            "review queued".to_string(),
        );
        app_state.message_queue.queue(
            ChatContextType::Merge,
            merge_task.id.as_str(),
            "merge queued".to_string(),
        );
        app_state.message_queue.queue(
            ChatContextType::Ideation,
            session.id.as_str(),
            "ideation queued".to_string(),
        );

        let count = count_slot_consuming_queued_messages_for_project(&app_state, &project.id)
            .await
            .expect("count queued messages");

        assert_eq!(count, 3);
    }
}
