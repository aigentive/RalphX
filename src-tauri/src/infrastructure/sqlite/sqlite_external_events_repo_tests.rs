use super::sqlite_external_events_repo::SqliteExternalEventsRepository;
use crate::domain::repositories::ExternalEventsRepository;

#[tokio::test]
async fn insert_event_once_for_attempt_is_atomic_per_agent_run() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "CREATE TABLE external_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                project_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                created_at TEXT DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );",
        )
        .unwrap();
    let repo = SqliteExternalEventsRepository::new(connection);

    let first = repo
        .insert_event_once_for_attempt(
            "task:execution_completed",
            "project-1",
            "run-1",
            r#"{"agent_run_id":"run-1"}"#,
        )
        .await
        .unwrap();
    let duplicate = repo
        .insert_event_once_for_attempt(
            "task:execution_completed",
            "project-1",
            "run-1",
            r#"{"agent_run_id":"run-1"}"#,
        )
        .await
        .unwrap();
    let next_attempt = repo
        .insert_event_once_for_attempt(
            "task:execution_completed",
            "project-1",
            "run-2",
            r#"{"agent_run_id":"run-2"}"#,
        )
        .await
        .unwrap();

    assert!(first);
    assert!(!duplicate);
    assert!(next_attempt);
    let events = repo
        .get_events_after_cursor(&["project-1".to_string()], 0, 10)
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
}
