use std::sync::Arc;

use crate::application::agent_workspace_publish_lease::{
    begin_publish_operation_scope, publish_operation_lease_is_live,
    spawn_publish_operation_lease_heartbeat_for_scope, stop_publish_operation_lease_heartbeat,
};
use crate::application::AppState;
use crate::domain::entities::ChatConversationId;

#[tokio::test]
async fn dropped_operation_lease_is_not_revived_by_a_later_conversation_scope() {
    let state = AppState::new_test();
    let conversation_id =
        ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string());
    let first_operation = begin_publish_operation_scope(&conversation_id);

    spawn_publish_operation_lease_heartbeat_for_scope(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
        "first-operation-token".to_string(),
        &first_operation,
    );
    assert!(publish_operation_lease_is_live(
        &conversation_id,
        Some("first-operation-token")
    ));

    drop(first_operation);
    let _second_operation = begin_publish_operation_scope(&conversation_id);

    assert!(!publish_operation_lease_is_live(
        &conversation_id,
        Some("first-operation-token")
    ));
}

#[tokio::test]
async fn nested_scopes_keep_their_shared_operation_lease_live_until_all_drop() {
    let state = AppState::new_test();
    let conversation_id =
        ChatConversationId::from_string("22222222-2222-2222-2222-222222222222".to_string());
    let outer_operation = begin_publish_operation_scope(&conversation_id);
    let nested_operation = outer_operation.nested();

    spawn_publish_operation_lease_heartbeat_for_scope(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
        "nested-operation-token".to_string(),
        &outer_operation,
    );
    drop(outer_operation);

    assert!(publish_operation_lease_is_live(
        &conversation_id,
        Some("nested-operation-token")
    ));

    drop(nested_operation);

    assert!(!publish_operation_lease_is_live(
        &conversation_id,
        Some("nested-operation-token")
    ));
}

#[tokio::test]
async fn heartbeat_requires_its_matching_operation_scope_and_token() {
    let state = AppState::new_test();
    let first_conversation =
        ChatConversationId::from_string("33333333-3333-3333-3333-333333333333".to_string());
    let second_conversation =
        ChatConversationId::from_string("44444444-4444-4444-4444-444444444444".to_string());
    let operation = begin_publish_operation_scope(&first_conversation);

    assert!(!publish_operation_lease_is_live(&first_conversation, None));
    assert!(!publish_operation_lease_is_live(
        &first_conversation,
        Some("missing-heartbeat-token")
    ));

    spawn_publish_operation_lease_heartbeat_for_scope(
        Arc::clone(&state.agent_conversation_workspace_repo),
        second_conversation.clone(),
        "mismatched-operation-token".to_string(),
        &operation,
    );

    assert!(!publish_operation_lease_is_live(
        &second_conversation,
        Some("mismatched-operation-token")
    ));
    assert!(!publish_operation_lease_is_live(
        &first_conversation,
        Some("mismatched-operation-token")
    ));
}

#[tokio::test]
async fn stop_heartbeat_only_removes_its_matching_token() {
    let state = AppState::new_test();
    let conversation_id =
        ChatConversationId::from_string("55555555-5555-5555-5555-555555555555".to_string());
    let operation = begin_publish_operation_scope(&conversation_id);

    spawn_publish_operation_lease_heartbeat_for_scope(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation_id.clone(),
        "current-token".to_string(),
        &operation,
    );
    stop_publish_operation_lease_heartbeat(&conversation_id, "stale-token");
    assert!(publish_operation_lease_is_live(
        &conversation_id,
        Some("current-token")
    ));

    stop_publish_operation_lease_heartbeat(&conversation_id, "current-token");
    assert!(!publish_operation_lease_is_live(
        &conversation_id,
        Some("current-token")
    ));
}
