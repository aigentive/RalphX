use super::SendMessageOptions;
use crate::application::AppState;
use crate::domain::entities::{
    ChatContextType, ChatConversation, CoordinationMode, ProjectId, TeamIntent,
};

#[tokio::test]
async fn explicit_team_intent_persists_coordination_mode_for_existing_conversation() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-team-send".to_string());
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project_id.clone()))
        .await
        .expect("conversation should persist");
    let service = state.build_chat_service();

    let (resolved, created) = service
        .get_or_create_conversation_for_send(
            ChatContextType::Project,
            project_id.as_str(),
            &SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                team_intent: Some(TeamIntent::rx_native(None)),
                ..Default::default()
            },
        )
        .await
        .expect("conversation should resolve");

    assert!(!created);
    assert_eq!(resolved.coordination_mode, CoordinationMode::RxNativeTeam);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation should load")
        .expect("conversation should exist");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}

#[tokio::test]
async fn standalone_send_rejects_team_intent_without_flipping_coordination_mode() {
    let state = AppState::new_test();
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_standalone())
        .await
        .expect("standalone conversation should persist");
    let service = state.build_chat_service();

    let error = service
        .get_or_create_conversation_for_send(
            ChatContextType::Standalone,
            &conversation.context_id,
            &SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                team_intent: Some(TeamIntent::rx_native(None)),
                ..Default::default()
            },
        )
        .await
        .expect_err("standalone send must reject team intent");

    assert!(error
        .to_string()
        .contains("Only project agent conversations can change capabilities"));
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation should load")
        .expect("conversation should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::Solo);
}

#[tokio::test]
async fn team_send_rejects_solo_downgrade_without_touching_persisted_mode() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-team-downgrade-guard".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("Team conversation should persist");
    let service = state.build_chat_service();

    let error = service
        .get_or_create_conversation_for_send(
            ChatContextType::Project,
            project_id.as_str(),
            &SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                team_intent: Some(TeamIntent {
                    coordination_mode: CoordinationMode::Solo,
                    strategy: None,
                }),
                ..Default::default()
            },
        )
        .await
        .expect_err("Solo intent on a Team send must fail closed");

    assert!(
        error
            .to_string()
            .contains("Leaving Team mode requires the capability change action"),
        "unexpected error: {error}"
    );
    // The guard must fail before the raw update_coordination_mode write ever runs:
    // the persisted conversation must still read back as RxNativeTeam.
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation should load")
        .expect("conversation should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}

#[tokio::test]
async fn team_send_allows_rx_native_replay_without_error() {
    let state = AppState::new_test();
    let project_id = ProjectId::from_string("project-team-replay-guard".to_string());
    let mut conversation = ChatConversation::new_project(project_id.clone());
    conversation.coordination_mode = CoordinationMode::RxNativeTeam;
    let conversation = state
        .chat_conversation_repo
        .create(conversation)
        .await
        .expect("Team conversation should persist");
    let service = state.build_chat_service();

    let (resolved, created) = service
        .get_or_create_conversation_for_send(
            ChatContextType::Project,
            project_id.as_str(),
            &SendMessageOptions {
                conversation_id_override: Some(conversation.id),
                team_intent: Some(TeamIntent::rx_native(None)),
                ..Default::default()
            },
        )
        .await
        .expect("rx_native intent on an existing Team conversation must not be rejected");

    assert!(!created);
    assert_eq!(resolved.coordination_mode, CoordinationMode::RxNativeTeam);
    let stored = state
        .chat_conversation_repo
        .get_by_id(&conversation.id)
        .await
        .expect("conversation should load")
        .expect("conversation should remain persisted");
    assert_eq!(stored.coordination_mode, CoordinationMode::RxNativeTeam);
}
