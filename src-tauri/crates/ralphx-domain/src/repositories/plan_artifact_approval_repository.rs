use async_trait::async_trait;

use crate::entities::ideation::PlanArtifactBundle;
use crate::entities::{Artifact, ArtifactId, IdeationSessionId};
use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanApprovalActor {
    User,
    Judge,
    PlanImport,
}

impl PlanApprovalActor {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Judge => "judge",
            Self::PlanImport => "plan_import",
        }
    }
}

impl std::fmt::Display for PlanApprovalActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanArtifactApproval {
    pub session_id: IdeationSessionId,
    pub artifact_id: ArtifactId,
    pub artifact_version: u32,
    pub blueprint_artifact_id: Option<ArtifactId>,
    pub blueprint_artifact_version: Option<u32>,
    pub approved_at: String,
    pub approved_by: String,
}

impl PlanArtifactApproval {
    pub fn matches_bundle(&self, bundle: &PlanArtifactBundle) -> bool {
        self.artifact_id == bundle.overview_id && self.blueprint_artifact_id == bundle.blueprint_id
    }

    pub fn matches_artifacts(&self, overview: &Artifact, blueprint: Option<&Artifact>) -> bool {
        self.artifact_id == overview.id
            && self.artifact_version == overview.metadata.version
            && self.blueprint_artifact_id.as_ref() == blueprint.map(|artifact| &artifact.id)
            && self.blueprint_artifact_version
                == blueprint.map(|artifact| artifact.metadata.version)
    }
}

#[async_trait]
pub trait PlanArtifactApprovalRepository: Send + Sync {
    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanArtifactApproval>>;

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<usize>;
}
