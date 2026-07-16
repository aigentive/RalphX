use super::ChatRuntimeFactoryDeps;
use crate::application::AppState;

#[test]
fn app_state_chat_factory_dependencies_include_persona_and_manual_role_defaults() {
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
}
