use super::{
    build_task_scheduler_from_deps, ChatRuntimeFactoryDeps, ExecutionState, RuntimeFactoryDeps,
};
use crate::application::AppState;
use std::sync::Arc;

#[test]
fn app_state_chat_factory_dependencies_include_persona_repository() {
    let state = AppState::new_test();

    let deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.persona_repo.is_some(),
        "handler-built chat services must retain AppState persona resolution"
    );
}

#[test]
fn app_state_scheduler_factory_dependencies_include_completion_authority_repositories() {
    let state = AppState::new_test();
    let deps = RuntimeFactoryDeps::from_app_state(&state);

    let scheduler = build_task_scheduler_from_deps(None, Arc::new(ExecutionState::new()), &deps);

    assert!(
        scheduler.task_step_repo.is_some(),
        "scheduler-built worker services must retain canonical task steps"
    );
    assert!(
        scheduler.validation_run_repo.is_some(),
        "scheduler-built worker services must retain authoritative validation runs"
    );
}
