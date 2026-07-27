use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{ArtifactId, IdeationSessionId};
use crate::domain::repositories::{
    PlanApprovalActor, PlanArtifactApproval, PlanArtifactApprovalRepository,
};
use crate::error::AppResult;

#[derive(Default)]
pub struct MemoryPlanArtifactApprovalRepository {
    approvals: RwLock<HashMap<String, PlanArtifactApproval>>,
}

impl MemoryPlanArtifactApprovalRepository {
    pub fn new() -> Self {
        Self {
            approvals: RwLock::new(HashMap::new()),
        }
    }

    pub fn approve(
        &self,
        session_id: IdeationSessionId,
        artifact_id: ArtifactId,
        artifact_version: u32,
        approved_by: PlanApprovalActor,
    ) {
        let approval = PlanArtifactApproval {
            session_id: session_id.clone(),
            artifact_id,
            artifact_version,
            blueprint_artifact_id: None,
            blueprint_artifact_version: None,
            approved_at: Utc::now().to_rfc3339(),
            approved_by: approved_by.as_str().to_string(),
        };
        self.approvals
            .write()
            .unwrap()
            .insert(session_id.as_str().to_string(), approval);
    }

    pub fn approve_bundle(
        &self,
        session_id: IdeationSessionId,
        artifact_id: ArtifactId,
        blueprint_artifact_id: ArtifactId,
        artifact_version: u32,
        approved_by: PlanApprovalActor,
    ) {
        let approval = PlanArtifactApproval {
            session_id: session_id.clone(),
            artifact_id,
            artifact_version,
            blueprint_artifact_id: Some(blueprint_artifact_id),
            blueprint_artifact_version: Some(artifact_version),
            approved_at: Utc::now().to_rfc3339(),
            approved_by: approved_by.as_str().to_string(),
        };
        self.approvals
            .write()
            .unwrap()
            .insert(session_id.as_str().to_string(), approval);
    }
}

#[async_trait]
impl PlanArtifactApprovalRepository for MemoryPlanArtifactApprovalRepository {
    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<PlanArtifactApproval>> {
        Ok(self
            .approvals
            .read()
            .unwrap()
            .get(session_id.as_str())
            .cloned())
    }

    async fn delete_by_session(&self, session_id: &IdeationSessionId) -> AppResult<usize> {
        Ok(self
            .approvals
            .write()
            .unwrap()
            .remove(session_id.as_str())
            .map(|_| 1)
            .unwrap_or(0))
    }
}
