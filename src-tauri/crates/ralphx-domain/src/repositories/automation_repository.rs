use async_trait::async_trait;

use crate::entities::{Automation, AutomationId, AutomationStatus, ProjectId};
use crate::error::AppResult;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationSettingsPatch {
    pub name: Option<String>,
    pub max_runs: Option<i64>,
    pub max_consecutive_failures: Option<i64>,
}

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn create(&self, automation: Automation) -> AppResult<Automation>;

    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>>;

    async fn list(&self, project_id: Option<ProjectId>) -> AppResult<Vec<Automation>>;

    async fn list_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Automation>>;

    async fn update_settings(
        &self,
        id: &AutomationId,
        patch: AutomationSettingsPatch,
    ) -> AppResult<Option<Automation>>;

    async fn update_goal_items_json(
        &self,
        id: &AutomationId,
        goal_items_json: Option<String>,
    ) -> AppResult<Option<Automation>>;

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool>;

    async fn delete_terminal(&self, id: &AutomationId) -> AppResult<bool>;
}
