use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use super::{helpers, v20260805180000_remote_conversation_lifecycle_requests};
use crate::domain::entities::{
    RemoteConversationLifecycleKind, RemoteConversationLifecycleRequest,
    RemoteConversationLifecycleStatus,
};
use crate::domain::repositories::RemoteConversationLifecycleRequestRepository;
use crate::infrastructure::sqlite::SqliteRemoteConversationLifecycleRequestRepository;

#[tokio::test]
async fn creates_columns_indexes_round_trips_and_is_idempotent() {
    let conn = Arc::new(Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
    {
        let conn = conn.lock().await;
        v20260805180000_remote_conversation_lifecycle_requests::migrate(&conn).unwrap();
        for column in [
            "id",
            "kind",
            "conversation_id",
            "close_pull_request",
            "allocated_conversation_id",
            "status",
            "error_code",
            "result_json",
            "claimed_at",
            "created_at",
            "updated_at",
        ] {
            assert!(helpers::column_exists(
                &conn,
                "remote_conversation_lifecycle_requests",
                column
            ));
        }
        let indexes = conn
            .prepare("PRAGMA index_list(remote_conversation_lifecycle_requests)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(indexes
            .iter()
            .any(|name| name == "idx_remote_conversation_lifecycle_pending"));
        assert!(indexes
            .iter()
            .any(|name| name == "idx_remote_conversation_lifecycle_conversation"));
        v20260805180000_remote_conversation_lifecycle_requests::migrate(&conn).unwrap();
    }

    let repo = SqliteRemoteConversationLifecycleRequestRepository::from_shared(Arc::clone(&conn));
    let now = Utc::now();
    let original = RemoteConversationLifecycleRequest {
        id: "lifecycle-round-trip".into(),
        kind: RemoteConversationLifecycleKind::Fork,
        conversation_id: "parent".into(),
        close_pull_request: false,
        allocated_conversation_id: Some("child".into()),
        status: RemoteConversationLifecycleStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    };
    repo.create_remote_conversation_lifecycle_request(original.clone())
        .await
        .unwrap();
    assert_eq!(repo.get(&original.id).await.unwrap().unwrap(), original);
}

#[test]
fn preserves_rows_when_migration_is_reapplied() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    v20260805180000_remote_conversation_lifecycle_requests::migrate(&conn).unwrap();
    conn.execute("INSERT INTO remote_conversation_lifecycle_requests(id,kind,conversation_id,status,created_at,updated_at) VALUES('legacy','archive','parent','pending','2026-08-05T00:00:00Z','2026-08-05T00:00:00Z')", []).unwrap();
    v20260805180000_remote_conversation_lifecycle_requests::migrate(&conn).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT kind FROM remote_conversation_lifecycle_requests WHERE id='legacy'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        "archive"
    );
}
