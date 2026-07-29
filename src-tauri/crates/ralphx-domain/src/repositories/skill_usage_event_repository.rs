use async_trait::async_trait;

use crate::domain::entities::types::ProjectId;
use crate::domain::entities::{ProjectSkillId, SkillUsageEvent};
use crate::error::AppResult;

#[derive(Debug, Clone, Default)]
pub struct SkillUsageListOptions {
    pub project_skill_id: Option<ProjectSkillId>,
    pub agent_run_id: Option<String>,
}

#[async_trait]
pub trait SkillUsageEventRepository: Send + Sync {
    async fn record(&self, event: SkillUsageEvent) -> AppResult<SkillUsageEvent>;

    async fn record_batch(&self, events: Vec<SkillUsageEvent>) -> AppResult<Vec<SkillUsageEvent>> {
        let mut saved = Vec::with_capacity(events.len());
        for event in events {
            saved.push(self.record(event).await?);
        }
        Ok(saved)
    }

    async fn list_by_project(
        &self,
        project_id: &ProjectId,
        options: SkillUsageListOptions,
    ) -> AppResult<Vec<SkillUsageEvent>>;
}
