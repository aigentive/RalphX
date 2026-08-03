use std::sync::Arc;

use crate::application::agent_workspace_publish_lease::{
    begin_publish_operation_scope, publish_operation_lease_is_live,
    spawn_publish_operation_lease_heartbeat_for_scope,
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
