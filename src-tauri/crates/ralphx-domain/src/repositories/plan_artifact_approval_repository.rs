use async_trait::async_trait;

use crate::entities::{ArtifactId, IdeationSessionId};
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
    pub approved_at: String,
    pub approved_by: String,
}

#[async_trait]
pub trait PlanArtifactApprovalRepository: Send + Sync {
    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanArtifactApproval>>;

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<usize>;
}
