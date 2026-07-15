use ralphx_lib::domain::entities::{ChatMessage, IdeationSessionId};
use ralphx_lib::domain::repositories::ChatMessageRepository;
use ralphx_lib::infrastructure::sqlite::{
    open_connection, run_migrations, SqliteChatMessageRepository,
};

fn setup_repo() -> SqliteChatMessageRepository {
    let conn = open_connection(&std::path::PathBuf::from(":memory:")).unwrap();
    run_migrations(&conn).unwrap();
    // Disable FK checks so we can insert messages without seeding a parent session row
    conn.execute("PRAGMA foreign_keys = OFF", []).unwrap();
    SqliteChatMessageRepository::new(conn)
}

// ==================== GET LATEST MESSAGE BY ROLE TESTS ====================

#[tokio::test]
async fn test_get_latest_message_by_role_returns_none_when_empty() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    let result = repo
        .get_latest_message_by_role(&session_id, "user")
        .await
        .unwrap();

    assert!(result.is_none(), "should return None when session has no messages");
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_none_when_role_not_present() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert a user message but query for "orchestrator"
    repo.create(ChatMessage::user_in_session(session_id.clone(), "hello"))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_none(), "should return None when no messages with the requested role exist");
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_only_message() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    let msg = repo
        .create(ChatMessage::orchestrator_in_session(session_id.clone(), "agent reply"))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_some(), "should return the single matching message");
    assert_eq!(result.unwrap().id, msg.id);
}

#[tokio::test]
async fn test_get_latest_message_by_role_returns_correct_latest_with_multiple_messages() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert two orchestrator messages with a brief delay so created_at differs
    let _first = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "first agent reply",
        ))
        .await
        .unwrap();

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    let second = repo
        .create(ChatMessage::orchestrator_in_session(
            session_id.clone(),
            "second agent reply",
        ))
        .await
        .unwrap();

    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_some());
    assert_eq!(
        result.unwrap().id,
        second.id,
        "should return the most recently created message"
    );
}

#[tokio::test]
async fn test_get_latest_message_by_role_filters_by_session() {
    let repo = setup_repo();
    let session_a = IdeationSessionId::new();
    let session_b = IdeationSessionId::new();

    // Insert a message in session_a
    repo.create(ChatMessage::orchestrator_in_session(session_a.clone(), "msg in a"))
        .await
        .unwrap();

    // Query session_b — should return None
    let result = repo
        .get_latest_message_by_role(&session_b, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_none(), "should only return messages belonging to the queried session");
}

#[tokio::test]
async fn test_get_latest_message_by_role_does_not_cross_roles() {
    let repo = setup_repo();
    let session_id = IdeationSessionId::new();

    // Insert a user message
    repo.create(ChatMessage::user_in_session(session_id.clone(), "user msg"))
        .await
        .unwrap();

    // Query for "orchestrator" — should return None
    let result = repo
        .get_latest_message_by_role(&session_id, "orchestrator")
        .await
        .unwrap();

    assert!(result.is_none(), "should not return messages of a different role");
}
