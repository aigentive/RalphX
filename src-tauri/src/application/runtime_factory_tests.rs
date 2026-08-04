use super::{ChatRuntimeFactoryDeps, RuntimeFactoryDeps};
use crate::application::AppState;

#[test]
fn app_state_chat_factory_dependencies_include_persona_manual_role_defaults_and_events() {
    let state = AppState::new_test();

    let deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.persona_repo.is_some(),
        "handler-built chat services must retain AppState persona resolution"
    );
    assert!(
        deps.manual_role_default_service.is_some(),
        "handler-built chat services must resolve backend-owned routing-role defaults"
    );
    assert!(
        deps.external_events_repo.is_some(),
        "background finalizers must retain durable completion-event delivery"
    );
    assert!(
        deps.plan_verification_completion.is_some(),
        "chat finalizers must receive typed Plan verification and approval settlement capability"
    );
    assert!(
        deps.atlassian_integration_service.is_some(),
        "handler-built chat services must retain Atlassian reference expansion"
    );
    assert!(
        deps.linear_integration_service.is_some(),
        "handler-built chat services must retain Linear reference expansion"
    );
    assert!(
        deps.granola_integration_service.is_some(),
        "handler-built chat services must retain Granola reference expansion"
    );
    assert!(
        deps.clickup_integration_service.is_some(),
        "handler-built chat services must retain ClickUp reference expansion"
    );
}

#[test]
fn app_state_runtime_factory_dependencies_include_completion_authority_repositories() {
    let state = AppState::new_test();

    let deps = RuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.task_step_repo.is_some(),
        "no-AppHandle scheduler paths must retain task-step completion authority"
    );
    assert!(
        deps.validation_run_repo.is_some(),
        "no-AppHandle scheduler paths must retain first-class validation authority"
    );
}
