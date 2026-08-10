use super::{helpers, run_migrations_through, v20260805170000_remote_recovery_action};
use crate::domain::repositories::RemoteTaskActionRequestRepository;
use crate::infrastructure::sqlite::SqliteRemoteTaskActionRequestRepository;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn adds_nullable_recovery_action_idempotently_and_round_trips_old_and_new_rows() {
    let conn = Arc::new(Mutex::new(rusqlite::Connection::open_in_memory().unwrap()));
    {
        let conn = conn.lock().await;
        run_migrations_through(&conn, 20260805160000).unwrap();
        conn.execute("INSERT INTO remote_resume_requests(id,family,action,task_id,project_id,force_restart,status,created_at,updated_at) VALUES('old','task','resume','task','project',0,'pending','2026-08-05T00:00:00Z','2026-08-05T00:00:00Z')", []).unwrap();

        v20260805170000_remote_recovery_action::migrate(&conn).unwrap();
        v20260805170000_remote_recovery_action::migrate(&conn).unwrap();

        assert!(helpers::column_exists(
            &conn,
            "remote_resume_requests",
            "recovery_action"
        ));
    }
    let repo = SqliteRemoteTaskActionRequestRepository::from_shared(Arc::clone(&conn));
    let old = repo.get("old").await.unwrap().unwrap();
    assert_eq!(
        old.action,
        crate::domain::entities::RemoteTaskAction::Resume
    );
    assert_eq!(old.recovery_action, None);

    conn.lock().await.execute("UPDATE remote_resume_requests SET action='resolveRecoveryPrompt', recovery_action='cancel' WHERE id='old'", []).unwrap();
    let new = repo.get("old").await.unwrap().unwrap();
    assert_eq!(
        new.action,
        crate::domain::entities::RemoteTaskAction::ResolveRecoveryPrompt
    );
    assert_eq!(
        new.recovery_action,
        Some(crate::domain::entities::RemoteRecoveryAction::Cancel)
    );
}
