// Tests for SqliteTeamMessageRepository

use super::sqlite_team_message_repo::SqliteTeamMessageRepository;
use crate::domain::entities::team::{TeamMessageId, TeamMessageRecord, TeamSessionId};
use crate::domain::repositories::TeamMessageRepository;
use crate::testing::SqliteTestDb;

fn setup_test_db() -> SqliteTestDb {
    SqliteTestDb::new("sqlite-team-message-repo")
}

fn create_test_session(db: &SqliteTestDb) -> TeamSessionId {
    let id = TeamSessionId::new();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO team_sessions (id, team_name, context_id, context_type, phase, teammate_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'), strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now'))",
            rusqlite::params![
                id.as_str(),
                "test-team",
                "ctx-1",
                "project",
                "forming",
                "[]",
            ],
        )
        .unwrap();
    });
    id
}

// ==================== CREATE TESTS ====================

#[tokio::test]
async fn test_create_returns_message() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg = TeamMessageRecord::new(session_id.clone(), "alice", "Hello team");
    let msg_id = msg.id.clone();

    let result = repo.create(msg).await;

    assert!(result.is_ok());
    let created = result.unwrap();
    assert_eq!(created.id, msg_id);
    assert_eq!(created.sender, "alice");
    assert_eq!(created.content, "Hello team");
    assert_eq!(created.team_session_id, session_id);
}

#[tokio::test]
async fn test_create_with_recipient_persists_fields() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let mut msg = TeamMessageRecord::new(session_id.clone(), "lead", "Task done");
    msg.recipient = Some("worker1".to_string());
    msg.message_type = "direct_message".to_string();
    let msg_id = msg.id.clone();

    repo.create(msg).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();
    let found = messages.iter().find(|m| m.id == msg_id).unwrap();
    assert_eq!(found.recipient, Some("worker1".to_string()));
    assert_eq!(found.message_type, "direct_message");
}

#[tokio::test]
async fn test_create_without_recipient_has_none() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg = TeamMessageRecord::new(session_id.clone(), "alice", "Broadcast message");
    let msg_id = msg.id.clone();

    repo.create(msg).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();
    let found = messages.iter().find(|m| m.id == msg_id).unwrap();
    assert!(found.recipient.is_none());
}

#[tokio::test]
async fn test_create_duplicate_id_fails() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg = TeamMessageRecord::new(session_id.clone(), "alice", "Hello");
    repo.create(msg.clone()).await.unwrap();

    let result = repo.create(msg).await;

    assert!(result.is_err());
}

// ==================== GET BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_by_session_returns_all_messages() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id.clone(), "alice", "Hello");
    let msg2 = TeamMessageRecord::new(session_id.clone(), "bob", "World");
    let msg3 = TeamMessageRecord::new(session_id.clone(), "alice", "Again");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();
    repo.create(msg3).await.unwrap();

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 3);
}

#[tokio::test]
async fn test_get_by_session_returns_empty_when_no_messages() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let result = repo.get_by_session(&session_id).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

#[tokio::test]
async fn test_get_by_session_filters_by_session() {
    let db = setup_test_db();
    let session_id1 = create_test_session(&db);
    let session_id2 = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id1.clone(), "alice", "Session 1 msg");
    let msg2 = TeamMessageRecord::new(session_id2.clone(), "bob", "Session 2 msg");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let messages = repo.get_by_session(&session_id1).await.unwrap();

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].team_session_id, session_id1);
}

#[tokio::test]
async fn test_get_by_session_ordered_asc_by_created_at() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id.clone(), "alice", "First");
    repo.create(msg1).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg2 = TeamMessageRecord::new(session_id.clone(), "bob", "Second");
    repo.create(msg2).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let msg3 = TeamMessageRecord::new(session_id.clone(), "alice", "Third");
    repo.create(msg3).await.unwrap();

    let messages = repo.get_by_session(&session_id).await.unwrap();

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "First");
    assert_eq!(messages[1].content, "Second");
    assert_eq!(messages[2].content, "Third");
}

// ==================== GET RECENT BY SESSION TESTS ====================

#[tokio::test]
async fn test_get_recent_by_session_limits_results() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    for i in 1..=5u32 {
        let msg = TeamMessageRecord::new(session_id.clone(), "alice", format!("Message {}", i));
        repo.create(msg).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let result = repo.get_recent_by_session(&session_id, 3).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 3);
}

#[tokio::test]
async fn test_get_recent_by_session_returns_latest_in_asc_order() {
    // Implementation: SQL ORDER BY DESC LIMIT N, then .reverse() → chronological order
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    for i in 1..=5u32 {
        let msg = TeamMessageRecord::new(session_id.clone(), "alice", format!("Message {}", i));
        repo.create(msg).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    let messages = repo.get_recent_by_session(&session_id, 2).await.unwrap();

    // Gets most recent 2 (DESC) then reverses → ascending order
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Message 4");
    assert_eq!(messages[1].content, "Message 5");
}

#[tokio::test]
async fn test_get_recent_by_session_returns_all_if_fewer_than_limit() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg = TeamMessageRecord::new(session_id.clone(), "alice", "Only one");
    repo.create(msg).await.unwrap();

    let messages = repo.get_recent_by_session(&session_id, 100).await.unwrap();

    assert_eq!(messages.len(), 1);
}

#[tokio::test]
async fn test_get_recent_by_session_returns_empty_for_no_messages() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let result = repo.get_recent_by_session(&session_id, 10).await;

    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ==================== COUNT BY SESSION TESTS ====================

#[tokio::test]
async fn test_count_by_session_returns_zero_when_empty() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let count = repo.count_by_session(&session_id).await.unwrap();

    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_count_by_session_counts_correctly() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    for i in 1..=4u32 {
        let msg = TeamMessageRecord::new(session_id.clone(), "alice", format!("Msg {}", i));
        repo.create(msg).await.unwrap();
    }

    let count = repo.count_by_session(&session_id).await.unwrap();

    assert_eq!(count, 4);
}

#[tokio::test]
async fn test_count_by_session_filters_by_session() {
    let db = setup_test_db();
    let session_id1 = create_test_session(&db);
    let session_id2 = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id1.clone(), "a", "S1-msg1");
    let msg2 = TeamMessageRecord::new(session_id1.clone(), "b", "S1-msg2");
    let msg3 = TeamMessageRecord::new(session_id2.clone(), "c", "S2-msg1");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();
    repo.create(msg3).await.unwrap();

    assert_eq!(repo.count_by_session(&session_id1).await.unwrap(), 2);
    assert_eq!(repo.count_by_session(&session_id2).await.unwrap(), 1);
}

// ==================== DELETE BY SESSION TESTS ====================

#[tokio::test]
async fn test_delete_by_session_removes_all_messages() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id.clone(), "alice", "Msg 1");
    let msg2 = TeamMessageRecord::new(session_id.clone(), "bob", "Msg 2");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    let result = repo.delete_by_session(&session_id).await;

    assert!(result.is_ok());
    let remaining = repo.get_by_session(&session_id).await.unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_delete_by_session_does_not_affect_other_sessions() {
    let db = setup_test_db();
    let session_id1 = create_test_session(&db);
    let session_id2 = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id1.clone(), "alice", "S1 msg");
    let msg2 = TeamMessageRecord::new(session_id2.clone(), "bob", "S2 msg");

    repo.create(msg1).await.unwrap();
    repo.create(msg2).await.unwrap();

    repo.delete_by_session(&session_id1).await.unwrap();

    let s1_msgs = repo.get_by_session(&session_id1).await.unwrap();
    let s2_msgs = repo.get_by_session(&session_id2).await.unwrap();

    assert!(s1_msgs.is_empty());
    assert_eq!(s2_msgs.len(), 1);
}

#[tokio::test]
async fn test_delete_by_session_for_nonexistent_session_succeeds() {
    let db = setup_test_db();
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let nonexistent_id = TeamSessionId::new();
    let result = repo.delete_by_session(&nonexistent_id).await;

    assert!(result.is_ok());
}

// ==================== DELETE SINGLE MESSAGE TESTS ====================

#[tokio::test]
async fn test_delete_removes_single_message() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let msg1 = TeamMessageRecord::new(session_id.clone(), "alice", "Keep");
    let msg2 = TeamMessageRecord::new(session_id.clone(), "bob", "Delete me");

    repo.create(msg1.clone()).await.unwrap();
    repo.create(msg2.clone()).await.unwrap();

    let result = repo.delete(&msg2.id).await;
    assert!(result.is_ok());

    let remaining = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, msg1.id);
}

#[tokio::test]
async fn test_delete_nonexistent_message_succeeds() {
    let db = setup_test_db();
    let repo = SqliteTeamMessageRepository::new(db.new_connection());

    let nonexistent_id = TeamMessageId::new();
    let result = repo.delete(&nonexistent_id).await;

    assert!(result.is_ok());
}

// ==================== FROM SHARED TESTS ====================

#[tokio::test]
async fn test_from_shared_creates_and_retrieves() {
    let db = setup_test_db();
    let session_id = create_test_session(&db);
    let shared_conn = db.shared_conn();
    let repo = SqliteTeamMessageRepository::from_shared(shared_conn);

    let msg = TeamMessageRecord::new(session_id.clone(), "alice", "Shared conn test");
    let result = repo.create(msg).await;

    assert!(result.is_ok());
    let messages = repo.get_by_session(&session_id).await.unwrap();
    assert_eq!(messages.len(), 1);
}
