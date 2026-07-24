use super::ideation_commands_apply::{
    phase_insert_execution_plan, recheck_exact_plan_verification,
};
use crate::application::AppState;
use crate::domain::entities::ideation::PLAN_CONTRACT_V2;
use crate::domain::entities::{Artifact, ArtifactType, IdeationSession, Project};

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

#[tokio::test]
async fn final_verification_recheck_rejects_stale_v2_blueprint_proof() {
    let state = AppState::new_sqlite_for_apply_test();
    let project = state
        .project_repo
        .create(Project::new(
            "Exact pair guard".to_string(),
            "/tmp/ralphx-exact-pair-guard".to_string(),
        ))
        .await
        .unwrap();
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Overview",
            ArtifactType::Specification,
            "Overview content",
            "test",
        ))
        .await
        .unwrap();
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Blueprint",
            ArtifactType::Specification,
            "Blueprint content",
            "test",
        ))
        .await
        .unwrap();
    let stale_blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Stale blueprint",
            ArtifactType::Specification,
            "Stale content",
            "test",
        ))
        .await
        .unwrap();
    let mut session = IdeationSession::new(project.id);
    session.plan_artifact_id = Some(overview.id.clone());
    session.plan_blueprint_artifact_id = Some(blueprint.id.clone());
    session.verified_plan_artifact_id = Some(overview.id.clone());
    session.verified_plan_blueprint_artifact_id = Some(stale_blueprint.id);
    session.plan_contract_version = PLAN_CONTRACT_V2;
    let session = state.ideation_session_repo.create(session).await.unwrap();

    let session_id = session.id.to_string();
    let expected_overview_id = overview.id.to_string();
    let expected_blueprint_id = blueprint.id.to_string();
    let error = state
        .db
        .run_transaction(move |conn| {
            recheck_exact_plan_verification(
                conn,
                &session_id,
                Some(&expected_overview_id),
                Some(&expected_blueprint_id),
                PLAN_CONTRACT_V2,
                true,
            )
        })
        .await
        .expect_err("a stale Blueprint proof must fail the transaction-final recheck");

    assert!(error.to_string().contains("lost exact verification proof"));
}
