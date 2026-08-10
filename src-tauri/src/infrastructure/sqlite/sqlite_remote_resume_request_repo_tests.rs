use chrono::{Duration, Utc};

use super::*;
use crate::domain::entities::{RemoteExecutionResumeRequest, RemoteResumeRequestStatus};
use crate::testing::SqliteTestDb;

fn row(id: &str) -> RemoteExecutionResumeRequest {
    let now = Utc::now();
    RemoteExecutionResumeRequest {
        id: id.into(),
        project_id: None,
        status: RemoteResumeRequestStatus::Pending,
        error_code: None,
        result: None,
        claimed_at: None,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn sqlite_claim_is_guarded_and_completion_is_terminal() {
    let db = SqliteTestDb::new("remote-resume-claim");
    let repo = SqliteRemoteExecutionResumeRequestRepository::from_shared(db.shared_conn());
    repo.create_execution_resume_request(row("one"))
        .await
        .expect("create");
    let claimed = repo
        .claim_pending(Utc::now())
        .await
        .expect("claim")
        .expect("row");
    assert_eq!(claimed.status, RemoteResumeRequestStatus::Starting);
    assert!(repo
        .claim_pending(Utc::now())
        .await
        .expect("second claim")
        .is_none());
    repo.complete("one", serde_json::json!({"success":true}), Utc::now())
        .await
        .expect("complete");
    let settled = repo.get("one").await.expect("read").expect("row");
    assert_eq!(settled.status, RemoteResumeRequestStatus::Completed);
    assert_eq!(settled.result, Some(serde_json::json!({"success":true})));
}

#[tokio::test]
async fn sqlite_stale_sweep_never_returns_claim_to_pending() {
    let db = SqliteTestDb::new("remote-resume-stale");
    let repo = SqliteRemoteExecutionResumeRequestRepository::from_shared(db.shared_conn());
    repo.create_execution_resume_request(row("stale"))
        .await
        .expect("create");
    repo.claim_pending(Utc::now() - Duration::minutes(10))
        .await
        .expect("claim");
    assert_eq!(
        repo.fail_stale(Utc::now() - Duration::minutes(5), Utc::now())
            .await
            .expect("sweep"),
        1
    );
    assert!(repo
        .claim_pending(Utc::now())
        .await
        .expect("claim after sweep")
        .is_none());
    assert_eq!(
        repo.get("stale").await.expect("read").expect("row").status,
        RemoteResumeRequestStatus::FailedStale
    );
}
