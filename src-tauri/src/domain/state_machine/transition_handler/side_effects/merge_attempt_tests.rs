use super::merge_attempt::append_source_update_failure_recovery_event;
use crate::domain::entities::task_metadata::{
    MergeFailureSource, MergeRecoveryMetadata, MergeRecoveryState,
};
use crate::domain::entities::{ProjectId, Task};

#[test]
fn source_update_failure_recovery_event_tracks_context_and_attempts() {
    let mut task = Task::new(
        ProjectId::from_string("proj-1".to_string()),
        "Merge task".to_string(),
    );

    append_source_update_failure_recovery_event(&mut task, "fetch failed", "task/source", "main");
    append_source_update_failure_recovery_event(
        &mut task,
        "lock contention",
        "task/source",
        "main",
    );

    let recovery = MergeRecoveryMetadata::from_task_metadata(task.metadata.as_deref())
        .expect("parse merge recovery")
        .expect("merge recovery should be present");
    assert_eq!(recovery.last_state, MergeRecoveryState::Failed);
    assert_eq!(recovery.events.len(), 2);
    assert_eq!(recovery.events[0].attempt, Some(1));
    assert_eq!(recovery.events[1].attempt, Some(2));
    assert_eq!(
        recovery.events[1].failure_source,
        Some(MergeFailureSource::TransientGit)
    );
    assert_eq!(
        recovery.events[1].source_branch.as_deref(),
        Some("task/source")
    );
    assert_eq!(recovery.events[1].target_branch.as_deref(), Some("main"));
    assert!(recovery.events[1]
        .message
        .contains("Source branch update failed"));
}
