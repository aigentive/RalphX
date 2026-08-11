use chrono::Utc;
use rusqlite::params;

use crate::domain::entities::{ArtifactId, IdeationSession, IdeationSessionId};
use crate::domain::repositories::PlanArtifactApprovalRepository;
use crate::infrastructure::sqlite::DbConnection;
use crate::testing::SqliteTestDb;

use super::SqlitePlanArtifactApprovalRepository;

fn setup_repo() -> (
    SqliteTestDb,
    IdeationSessionId,
    SqlitePlanArtifactApprovalRepository,
) {
    let db = SqliteTestDb::new("sqlite_plan_artifact_approval_repo_tests");
    let project = db.seed_project("Project 1");
    let mut session = IdeationSession::builder()
        .project_id(project.id)
        .title("Plan session")
        .build();
    session.id = IdeationSessionId::from_string("session-plan");
    let session_id = session.id.clone();
    db.insert_ideation_session(session);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO artifacts (
                id, type, name, content_type, content_text, created_by, version, created_at
            ) VALUES (
                'artifact-plan', 'specification', 'Run plan', 'inline', 'plan body',
                'tester', 7, ?1
            )",
            [Utc::now().to_rfc3339()],
        )
        .expect("insert artifact");
    });

    let repo =
        SqlitePlanArtifactApprovalRepository::new(DbConnection::from_shared(db.shared_conn()));
    (db, session_id, repo)
}

#[tokio::test]
async fn sqlite_plan_artifact_approval_repo_round_trips_approved_row() {
    let (db, session_id, repo) = setup_repo();
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO plan_artifact_approvals (
                session_id, artifact_id, artifact_version, approved_at, approved_by
            ) VALUES (?1, 'artifact-plan', 7, ?2, 'judge')",
            params![session_id.as_str(), Utc::now().to_rfc3339()],
        )
        .expect("insert approval");
    });

    let approval = repo
        .get_by_session(&session_id)
        .await
        .expect("load approval")
        .expect("approval should exist");

    assert_eq!(approval.session_id, session_id);
    assert_eq!(
        approval.artifact_id,
        ArtifactId::from_string("artifact-plan")
    );
    assert_eq!(approval.artifact_version, 7);
    assert_eq!(approval.approved_by, "judge");
    assert!(repo
        .get_by_session(&IdeationSessionId::from_string("missing-session"))
        .await
        .expect("missing lookup")
        .is_none());
}

#[tokio::test]
async fn sqlite_plan_artifact_approval_repo_deletes_only_the_requested_session() {
    let (db, session_id, repo) = setup_repo();
    let mut other_session = IdeationSession::builder()
        .project_id(db.seed_project("Project 2").id)
        .title("Other plan session")
        .build();
    other_session.id = IdeationSessionId::from_string("session-other");
    db.insert_ideation_session(other_session);
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO artifacts (
                id, type, name, content_type, content_text, created_by, version, created_at
            ) VALUES (
                'artifact-other', 'specification', 'Other plan', 'inline', 'other body',
                'tester', 2, ?1
            )",
            [Utc::now().to_rfc3339()],
        )
        .expect("insert other artifact");
        conn.execute(
            "INSERT INTO plan_artifact_approvals (
                session_id, artifact_id, artifact_version, approved_at, approved_by
            ) VALUES
                (?1, 'artifact-plan', 7, ?2, 'user'),
                ('session-other', 'artifact-other', 2, ?2, 'judge')",
            params![session_id.as_str(), Utc::now().to_rfc3339()],
        )
        .expect("insert approvals");
    });

    assert_eq!(
        repo.delete_by_session(&session_id)
            .await
            .expect("delete approval"),
        1
    );
    assert!(repo
        .get_by_session(&session_id)
        .await
        .expect("load deleted approval")
        .is_none());
    let remaining = repo
        .get_by_session(&IdeationSessionId::from_string("session-other"))
        .await
        .expect("load other approval")
        .expect("other approval remains");
    assert_eq!(remaining.artifact_version, 2);
    assert_eq!(
        repo.delete_by_session(&IdeationSessionId::from_string("missing-session"))
            .await
            .expect("delete missing approval"),
        0
    );
}
