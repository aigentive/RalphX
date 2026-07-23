use std::sync::Arc;

use crate::domain::entities::{ProjectId, Task, TaskOutcomeClass};
use crate::domain::repositories::{TaskOutcomeListOptions, TaskOutcomeRepository};
use crate::infrastructure::memory::MemoryTaskOutcomeRepository;

use super::merge_failure_outcomes::record_merge_failure_outcome;

#[tokio::test]
async fn merge_failure_outcomes_are_idempotent_per_attempt_and_comparable_across_attempts() {
    let repo = Arc::new(MemoryTaskOutcomeRepository::new());
    let task = Task::new(
        ProjectId::from_string("project-1".to_string()),
        "Merge task".to_string(),
    );

    let first = record_merge_failure_outcome(
        repo.clone(),
        &task,
        TaskOutcomeClass::MergeConflict,
        1,
        serde_json::json!({"error": "conflict in /Users/a/project/src/lib.rs"}),
        "conflict in /Users/a/project/src/lib.rs",
    )
    .await
    .unwrap();
    let replay = record_merge_failure_outcome(
        repo.clone(),
        &task,
        TaskOutcomeClass::MergeConflict,
        1,
        serde_json::json!({"error": "conflict in C:\\work\\project\\src\\lib.rs"}),
        "conflict in C:\\work\\project\\src\\lib.rs",
    )
    .await
    .unwrap();
    let later = record_merge_failure_outcome(
        repo.clone(),
        &task,
        TaskOutcomeClass::MergeConflict,
        2,
        serde_json::json!({"error": "conflict in /tmp/project/src/lib.rs"}),
        "conflict in /tmp/project/src/lib.rs",
    )
    .await
    .unwrap();

    assert_eq!(first.id, replay.id);
    assert_ne!(first.id, later.id);
    assert_eq!(first.failure_fingerprint, replay.failure_fingerprint);
    assert_eq!(first.failure_fingerprint, later.failure_fingerprint);
    assert_eq!(later.evidence_json["attempt"], 2);
    assert_eq!(later.outcome_class, Some(TaskOutcomeClass::MergeConflict));
    assert_eq!(later.task_id.as_deref(), Some(task.id.as_str()));

    let outcomes = repo
        .list_by_project(&task.project_id, TaskOutcomeListOptions::default())
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 2);
}
