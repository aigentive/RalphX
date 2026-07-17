use super::ideation_commands_apply::phase_insert_execution_plan;
use crate::application::AppState;
use crate::domain::entities::{IdeationSession, Project};

#[tokio::test]
async fn execution_plan_insert_is_at_most_once_per_active_session() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Duplicate start guard".to_string(),
            "/tmp/ralphx-duplicate-start-guard".to_string(),
        ))
        .await
        .unwrap();
    let session = state
        .ideation_session_repo
        .create(IdeationSession::new(project.id))
        .await
        .unwrap();
    let first_session_id = session.id.as_str().to_string();
    state
        .db
        .run_transaction(move |conn| {
            phase_insert_execution_plan(conn, &first_session_id).map(|_| ())
        })
        .await
        .unwrap();

    let second_session_id = session.id.as_str().to_string();
    let error = state
        .db
        .run_transaction(move |conn| {
            phase_insert_execution_plan(conn, &second_session_id).map(|_| ())
        })
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("already has an active execution plan"));
    assert_eq!(
        state
            .execution_plan_repo
            .get_by_session(&session.id)
            .await
            .unwrap()
            .len(),
        1,
    );
}
