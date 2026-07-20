use super::*;
use crate::domain::entities::team::TeamMessageRecord;

#[tokio::test]
async fn test_create_and_get_by_session() {
    let repo = MemoryTeamMessageRepository::new();
    let session_id = TeamSessionId::new();
    let msg = TeamMessageRecord::new(session_id.clone(), "worker", "hello");

    repo.create(msg).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].content, "hello");
}

#[tokio::test]
async fn test_count_by_session() {
    let repo = MemoryTeamMessageRepository::new();
    let session_id = TeamSessionId::new();

    repo.create(TeamMessageRecord::new(session_id.clone(), "a", "msg1"))
        .await
        .unwrap();
    repo.create(TeamMessageRecord::new(session_id.clone(), "b", "msg2"))
        .await
        .unwrap();

    assert_eq!(repo.count_by_session(&session_id).await.unwrap(), 2);
}

#[tokio::test]
async fn test_delete_by_session() {
    let repo = MemoryTeamMessageRepository::new();
    let session_id = TeamSessionId::new();

    repo.create(TeamMessageRecord::new(session_id.clone(), "a", "msg1"))
        .await
        .unwrap();
    repo.delete_by_session(&session_id).await.unwrap();

    assert_eq!(repo.count_by_session(&session_id).await.unwrap(), 0);
}

#[tokio::test]
async fn test_delete_single() {
    let repo = MemoryTeamMessageRepository::new();
    let session_id = TeamSessionId::new();
    let msg = TeamMessageRecord::new(session_id.clone(), "a", "msg1");
    let msg_id = msg.id.clone();

    repo.create(msg).await.unwrap();
    repo.delete(&msg_id).await.unwrap();

    assert_eq!(repo.count_by_session(&session_id).await.unwrap(), 0);
}

#[tokio::test]
async fn test_get_recent_by_session() {
    let repo = MemoryTeamMessageRepository::new();
    let session_id = TeamSessionId::new();

    for i in 1..=5 {
        repo.create(TeamMessageRecord::new(
            session_id.clone(),
            "sender",
            format!("msg {}", i),
        ))
        .await
        .unwrap();
    }

    let recent = repo.get_recent_by_session(&session_id, 3).await.unwrap();
    assert_eq!(recent.len(), 3);
}
