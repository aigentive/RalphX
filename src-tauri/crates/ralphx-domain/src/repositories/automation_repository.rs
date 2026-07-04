use async_trait::async_trait;

use crate::entities::{Automation, AutomationId, AutomationStatus, ProjectId};
use crate::error::AppResult;

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn create(&self, automation: Automation) -> AppResult<Automation>;

    async fn get_by_id(&self, id: &AutomationId) -> AppResult<Option<Automation>>;

    async fn list_by_project(&self, project_id: &ProjectId) -> AppResult<Vec<Automation>>;

    async fn compare_and_swap_status(
        &self,
        id: &AutomationId,
        from: AutomationStatus,
        to: AutomationStatus,
        paused_reason_code: Option<String>,
        paused_reason_detail: Option<String>,
    ) -> AppResult<bool>;
}
