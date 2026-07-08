use async_trait::async_trait;

use crate::entities::{ArtifactId, IdeationSessionId};
use crate::error::AppResult;

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
}
