use super::ChatRuntimeFactoryDeps;
use crate::application::AppState;

#[test]
fn app_state_chat_factory_dependencies_include_persona_repository() {
    let state = AppState::new_test();

    let deps = ChatRuntimeFactoryDeps::from_app_state(&state);

    assert!(
        deps.persona_repo.is_some(),
        "handler-built chat services must retain AppState persona resolution"
    );
}
