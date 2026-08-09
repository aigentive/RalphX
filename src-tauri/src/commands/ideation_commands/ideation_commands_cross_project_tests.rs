use crate::application::AppState;
use crate::commands::ideation_commands::ideation_commands_types::CreateCrossProjectSessionInput;
use crate::domain::entities::ideation::{PLAN_CONTRACT_V1, PLAN_CONTRACT_V2};
use crate::domain::entities::{
    Artifact, ArtifactId, ArtifactType, IdeationSession, ProjectId, VerificationStatus,
};

use super::ideation_commands_cross_project::{
    create_cross_project_session_impl, require_importable_plan_bundle,
};

#[test]
fn cross_project_import_rejects_grandfathered_v1_source() {
    let overview_id = ArtifactId::from_string("legacy-overview");
    let mut source = IdeationSession::builder()
        .project_id(ProjectId::from_string("source-project".to_string()))
        .plan_artifact_id(overview_id.clone())
        .plan_contract_version(PLAN_CONTRACT_V1)
        .build();
    source.verification_status = VerificationStatus::Verified;
    source.verified_plan_artifact_id = Some(overview_id);

    let error = require_importable_plan_bundle(&source)
        .expect_err("a legacy source must not mint a new imported v1 session");

    assert!(error.contains("complete v2 Overview and Blueprint bundle"));
}

#[tokio::test]
async fn cross_project_import_clones_the_verified_bundle_into_owned_pointers() {
    let state = AppState::new_sqlite_test();
    let temp = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("temporary project root should be created");
    let target_path = temp.path().join("target-project");
    std::fs::create_dir_all(&target_path).expect("target project directory should exist");
    let overview = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Source overview",
            ArtifactType::Specification,
            "# Overview",
            "test",
        ))
        .await
        .expect("source overview should persist");
    let blueprint = state
        .artifact_repo
        .create(Artifact::new_inline(
            "Source blueprint",
            ArtifactType::Specification,
            "# Blueprint",
            "test",
        ))
        .await
        .expect("source blueprint should persist");
    let mut source = IdeationSession::builder()
        .project_id(ProjectId::from_string("source-project".to_string()))
        .plan_artifact_id(overview.id.clone())
        .plan_blueprint_artifact_id(blueprint.id.clone())
        .plan_contract_version(PLAN_CONTRACT_V2)
        .build();
    source.verification_status = VerificationStatus::Verified;
    source.verified_plan_artifact_id = Some(overview.id.clone());
    source.verified_plan_blueprint_artifact_id = Some(blueprint.id.clone());
    let source = state
        .ideation_session_repo
        .create(source)
        .await
        .expect("source session should persist");
    let response = create_cross_project_session_impl(
        &state,
        state.events.as_ref(),
        CreateCrossProjectSessionInput {
            target_project_path: target_path.to_string_lossy().to_string(),
            source_session_id: source.id.as_str().to_string(),
            title: None,
        },
    )
    .await
    .expect("verified bundle should import");
    let imported = state
        .ideation_session_repo
        .get_by_id(&crate::domain::entities::IdeationSessionId::from_string(
            response.id,
        ))
        .await
        .expect("imported session lookup should succeed")
        .expect("imported session should exist");
    let imported_overview = imported
        .plan_artifact_id
        .expect("imported overview should be owned");
    let imported_blueprint = imported
        .plan_blueprint_artifact_id
        .expect("imported blueprint should be owned");

    assert_ne!(imported_overview, overview.id);
    assert_ne!(imported_blueprint, blueprint.id);
    assert!(imported.inherited_plan_artifact_id.is_none());
    assert!(imported.inherited_plan_blueprint_artifact_id.is_none());
    assert_eq!(imported.verified_plan_artifact_id, Some(imported_overview));
    assert_eq!(
        imported.verified_plan_blueprint_artifact_id,
        Some(imported_blueprint)
    );
}
