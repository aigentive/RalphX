use super::get_task_context_impl;
use crate::application::AppState;
use crate::domain::entities::{InternalStatus, ProjectId, Task};

#[tokio::test]
async fn get_task_context_impl_filters_resolved_blockers_and_keeps_active_ones() {
    let state = AppState::new_test();
    let project_id = ProjectId::new();

    let dependent = state
        .task_repo
        .create(Task::new(project_id.clone(), "Dependent".to_string()))
        .await
        .unwrap();

    let mut active_blocker = Task::new(project_id.clone(), "Active Blocker".to_string());
    active_blocker.internal_status = InternalStatus::Executing;
    let active_blocker = state.task_repo.create(active_blocker).await.unwrap();

    let mut merged_blocker = Task::new(project_id, "Merged Blocker".to_string());
    merged_blocker.internal_status = InternalStatus::Merged;
    let merged_blocker = state.task_repo.create(merged_blocker).await.unwrap();

    state
        .task_dependency_repo
        .add_dependency(&dependent.id, &active_blocker.id)
        .await
        .unwrap();
    state
        .task_dependency_repo
        .add_dependency(&dependent.id, &merged_blocker.id)
        .await
        .unwrap();

    let context = get_task_context_impl(&state, &dependent.id).await.unwrap();

    assert_eq!(context.blocked_by.len(), 1);
    assert_eq!(context.blocked_by[0].id, active_blocker.id);
    assert_eq!(context.tier, Some(2));
    assert!(
        context
            .context_hints
            .iter()
            .any(|hint| hint.contains("Active Blocker")),
        "active blockers should still be surfaced in HTTP task context hints"
    );
    assert!(
        !context
            .context_hints
            .iter()
            .any(|hint| hint.contains("Merged Blocker")),
        "resolved blockers must not be emitted as active HTTP context blockers"
    );
}
