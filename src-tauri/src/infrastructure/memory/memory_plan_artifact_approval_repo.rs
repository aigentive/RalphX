use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use chrono::Utc;

use crate::domain::entities::{ArtifactId, IdeationSessionId};
use crate::domain::repositories::{PlanArtifactApproval, PlanArtifactApprovalRepository};
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
        approved_by: &str,
    ) {
        let approval = PlanArtifactApproval {
            session_id: session_id.clone(),
            artifact_id,
            artifact_version,
            approved_at: Utc::now().to_rfc3339(),
            approved_by: approved_by.to_string(),
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
}
