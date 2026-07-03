use std::sync::Arc;

use crate::domain::entities::{
    ExecutionRecoveryMetadata, ExecutionRecoveryState, InternalStatus, Task, TaskId, TaskStepStatus,
};
use crate::domain::repositories::{TaskRepository, TaskStepRepository};
use crate::domain::state_machine::transition_handler::{parse_metadata, set_trigger_origin};
use crate::error::AppResult;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReadyRestartPreparation {
    pub cleared_failed_steps: u32,
}

pub async fn prepare_terminal_task_for_ready_restart(
    task_repo: &Arc<dyn TaskRepository>,
    task_step_repo: &Arc<dyn TaskStepRepository>,
    old_task: &Task,
    agent_variant: Option<&str>,
) -> AppResult<ReadyRestartPreparation> {
    if !old_task.internal_status.is_terminal() {
        return Ok(ReadyRestartPreparation::default());
    }

    let mut task_mut = old_task.clone();

    set_trigger_origin(&mut task_mut, "retry");

    task_mut.task_branch = None;
    task_mut.worktree_path = None;
    task_mut.merge_commit_sha = None;

    if let Ok(Some(mut recovery)) =
        ExecutionRecoveryMetadata::from_task_metadata(task_mut.metadata.as_deref())
    {
        recovery.stop_retrying = false;
        recovery.last_state = ExecutionRecoveryState::Retrying;
        recovery.events.clear();
        recovery.unrecoverable_reason = None;
        if let Ok(updated_meta) = recovery.update_task_metadata(task_mut.metadata.as_deref()) {
            task_mut.metadata = Some(updated_meta);
        }
    }

    let mut meta = parse_metadata(&task_mut).unwrap_or_else(|| serde_json::json!({}));
    if let Some(obj) = meta.as_object_mut() {
        if old_task.internal_status == InternalStatus::Failed {
            obj.insert("preserve_steps".to_string(), serde_json::json!(true));
        }
        match agent_variant {
            Some(variant) if !variant.is_empty() => {
                obj.insert("agent_variant".to_string(), serde_json::json!(variant));
            }
            _ => {
                obj.remove("agent_variant");
            }
        }
    }
    task_mut.metadata = Some(meta.to_string());

    task_repo.update(&task_mut).await?;

    let cleared_failed_steps = if old_task.internal_status == InternalStatus::Failed {
        clear_failed_steps_for_failed_restart(task_step_repo, &old_task.id).await?
    } else {
        0
    };

    Ok(ReadyRestartPreparation {
        cleared_failed_steps,
    })
}

pub async fn clear_failed_steps_for_failed_restart(
    task_step_repo: &Arc<dyn TaskStepRepository>,
    task_id: &TaskId,
) -> AppResult<u32> {
    let steps = task_step_repo.get_by_task(task_id).await?;

    let mut cleared = 0u32;
    for mut step in steps {
        if step.status != TaskStepStatus::Failed {
            continue;
        }

        step.status = TaskStepStatus::Pending;
        step.started_at = None;
        step.completed_at = None;
        step.completion_note = None;
        task_step_repo.update(&step).await?;
        cleared += 1;
    }

    Ok(cleared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::{ProjectId, TaskStep};
    use crate::infrastructure::memory::{MemoryTaskRepository, MemoryTaskStepRepository};

    #[tokio::test]
    async fn failed_ready_restart_clears_stale_refs_and_preserves_completed_steps() {
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MemoryTaskRepository::new());
        let task_step_repo: Arc<dyn TaskStepRepository> = Arc::new(MemoryTaskStepRepository::new());
        let project_id = ProjectId::from_string("project-restart".to_string());

        let mut task = Task::new(project_id, "Failed restart".to_string());
        task.internal_status = InternalStatus::Failed;
        task.task_branch = Some("task/stale".to_string());
        task.worktree_path = Some("/tmp/stale-worktree".to_string());
        task.merge_commit_sha = Some("deadbeef".to_string());
        task.metadata = Some(
            serde_json::json!({
                "execution_recovery": {
                    "last_state": "failed",
                    "stop_retrying": true,
                    "auto_recovery_count": 3,
                    "events": [{"kind": "attempt_failed"}],
                    "unrecoverable_reason": "missing work"
                },
                "agent_variant": "team"
            })
            .to_string(),
        );
        let task_id = task.id.clone();
        task_repo.create(task.clone()).await.unwrap();

        let mut completed_step = TaskStep::new(
            task_id.clone(),
            "completed".to_string(),
            0,
            "test".to_string(),
        );
        completed_step.status = TaskStepStatus::Completed;
        task_step_repo.create(completed_step).await.unwrap();

        let mut failed_step =
            TaskStep::new(task_id.clone(), "failed".to_string(), 1, "test".to_string());
        failed_step.status = TaskStepStatus::Failed;
        failed_step.started_at = Some(chrono::Utc::now());
        failed_step.completed_at = Some(chrono::Utc::now());
        failed_step.completion_note = Some("failed".to_string());
        task_step_repo.create(failed_step).await.unwrap();

        let preparation =
            prepare_terminal_task_for_ready_restart(&task_repo, &task_step_repo, &task, None)
                .await
                .unwrap();

        assert_eq!(preparation.cleared_failed_steps, 1);

        let updated_task = task_repo.get_by_id(&task_id).await.unwrap().unwrap();
        assert!(updated_task.task_branch.is_none());
        assert!(updated_task.worktree_path.is_none());
        assert!(updated_task.merge_commit_sha.is_none());

        let metadata: serde_json::Value =
            serde_json::from_str(updated_task.metadata.as_deref().unwrap()).unwrap();
        assert_eq!(metadata["trigger_origin"], "retry");
        assert_eq!(metadata["preserve_steps"], true);
        assert!(metadata.get("agent_variant").is_none());

        let steps = task_step_repo.get_by_task(&task_id).await.unwrap();
        assert_eq!(steps[0].status, TaskStepStatus::Completed);
        assert_eq!(steps[1].status, TaskStepStatus::Pending);
        assert!(steps[1].started_at.is_none());
        assert!(steps[1].completed_at.is_none());
        assert!(steps[1].completion_note.is_none());
    }
}
