use std::sync::Arc;

use ralphx_lib::application::chat_service::AppChatService;
use ralphx_lib::application::AppState;
use ralphx_lib::infrastructure::agents::claude::{
    reset_agent_personas_override_for_test, set_agent_personas_override,
};

struct PersonaFlagOverrideReset;

impl Drop for PersonaFlagOverrideReset {
    fn drop(&mut self) {
        reset_agent_personas_override_for_test();
    }
}

fn persona_flag_override_chat_service(state: &AppState) -> AppChatService {
    AppChatService::new(
        Arc::clone(&state.chat_message_repo),
        Arc::clone(&state.chat_attachment_repo),
        Arc::clone(&state.artifact_repo),
        Arc::clone(&state.chat_conversation_repo),
        Arc::clone(&state.agent_run_repo),
        Arc::clone(&state.project_repo),
        Arc::clone(&state.task_repo),
        Arc::clone(&state.task_dependency_repo),
        Arc::clone(&state.ideation_session_repo),
        Arc::clone(&state.delegated_session_repo),
        Arc::clone(&state.activity_event_repo),
        Arc::clone(&state.message_queue),
        Arc::clone(&state.running_agent_registry),
        Arc::clone(&state.memory_event_repo),
    )
}

#[test]
fn persona_flag_override_chat_service_keeps_builder_override_and_live_default() {
    let _reset = PersonaFlagOverrideReset;
    set_agent_personas_override(Some(true));
    let state = AppState::new_test();

    assert!(persona_flag_override_chat_service(&state).persona_feature_enabled_for_test());
    assert!(
        !persona_flag_override_chat_service(&state)
            .with_persona_feature_enabled(false)
            .persona_feature_enabled_for_test(),
        "the explicit test seam must override the live feature flag"
    );
}
