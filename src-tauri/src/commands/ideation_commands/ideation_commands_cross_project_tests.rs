use crate::domain::entities::ideation::PLAN_CONTRACT_V1;
use crate::domain::entities::{ArtifactId, IdeationSession, ProjectId, VerificationStatus};

use super::ideation_commands_cross_project::require_importable_plan_bundle;

#[test]
fn cross_project_import_rejects_grandfathered_v1_source() {
    let overview_id = ArtifactId::from_string("legacy-overview");
    let mut source = IdeationSession::builder()
        .project_id(ProjectId::from_string("source-project"))
        .plan_artifact_id(overview_id.clone())
        .plan_contract_version(PLAN_CONTRACT_V1)
        .build();
    source.verification_status = VerificationStatus::Verified;
    source.verified_plan_artifact_id = Some(overview_id);

    let error = require_importable_plan_bundle(&source)
        .expect_err("a legacy source must not mint a new imported v1 session");

    assert!(error.contains("complete v2 Overview and Blueprint bundle"));
}
