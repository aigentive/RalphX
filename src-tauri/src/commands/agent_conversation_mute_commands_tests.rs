use crate::application::AppState;
use crate::commands::agent_conversation_mute_commands::{
    set_agent_conversation_muted_for_app_state, SetAgentConversationMutedInput,
};
use crate::domain::entities::ChatConversation;

#[tokio::test]
async fn mute_command_persists_current_fingerprint_then_unmute_clears_it() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("conversation should be created");

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation.id.as_str().to_string(),
            muted: true,
        },
        &state,
    )
    .await
    .expect("mute should persist");
    assert!(state
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("mute lookup should succeed")
        .is_some());

    set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: conversation.id.as_str().to_string(),
            muted: false,
        },
        &state,
    )
    .await
    .expect("unmute should clear");
    assert!(state
        .agent_conversation_mute_repo
        .get_by_conversation_id(&conversation.id)
        .await
        .expect("mute lookup should succeed")
        .is_none());
}

#[tokio::test]
async fn mute_command_rejects_unknown_conversation() {
    let error = set_agent_conversation_muted_for_app_state(
        SetAgentConversationMutedInput {
            conversation_id: uuid::Uuid::new_v4().to_string(),
            muted: true,
        },
        &AppState::new_test(),
    )
    .await
    .expect_err("unknown conversation cannot be muted");

    assert!(error.contains("agent conversation not found"));
}
