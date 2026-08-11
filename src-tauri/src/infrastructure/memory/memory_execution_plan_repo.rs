// Memory-based ExecutionPlanRepository implementation for testing

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::domain::entities::{
    ExecutionPlan, ExecutionPlanHaltMode, ExecutionPlanId, ExecutionPlanStatus, IdeationSessionId,
};
use crate::domain::repositories::ExecutionPlanRepository;
use crate::error::{AppError, AppResult};

pub struct MemoryExecutionPlanRepository {
    plans: Arc<RwLock<HashMap<String, ExecutionPlan>>>,
}

impl Default for MemoryExecutionPlanRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryExecutionPlanRepository {
    pub fn new() -> Self {
        Self {
            plans: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl ExecutionPlanRepository for MemoryExecutionPlanRepository {
    async fn create(&self, plan: ExecutionPlan) -> AppResult<ExecutionPlan> {
        let mut plans = self.plans.write().await;
        plans.insert(plan.id.as_str().to_string(), plan.clone());
        Ok(plan)
    }

    async fn get_by_id(&self, id: &ExecutionPlanId) -> AppResult<Option<ExecutionPlan>> {
        let plans = self.plans.read().await;
        Ok(plans.get(id.as_str()).cloned())
    }

    async fn get_by_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Vec<ExecutionPlan>> {
        let plans = self.plans.read().await;
        let mut result: Vec<ExecutionPlan> = plans
            .values()
            .filter(|p| p.session_id == *session_id)
            .cloned()
            .collect();
        result.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(result)
    }

    async fn get_active_for_session(
        &self,
        session_id: &IdeationSessionId,
    ) -> AppResult<Option<ExecutionPlan>> {
        let plans = self.plans.read().await;
        Ok(plans
            .values()
            .filter(|p| p.session_id == *session_id && p.status == ExecutionPlanStatus::Active)
            .max_by_key(|p| p.created_at)
            .cloned())
    }

    async fn mark_superseded(&self, id: &ExecutionPlanId) -> AppResult<()> {
        let mut plans = self.plans.write().await;
        match plans.get_mut(id.as_str()) {
            Some(plan) => {
                plan.status = ExecutionPlanStatus::Superseded;
                Ok(())
            }
            None => Err(AppError::NotFound(format!(
                "Execution plan not found: {}",
                id
            ))),
        }
    }

    async fn set_halt_mode(
        &self,
        id: &ExecutionPlanId,
        halt_mode: ExecutionPlanHaltMode,
    ) -> AppResult<()> {
        let mut plans = self.plans.write().await;
        match plans.get_mut(id.as_str()) {
            Some(plan) => {
                plan.halt_mode = halt_mode;
                Ok(())
            }
            None => Err(AppError::NotFound(format!(
                "Execution plan not found: {}",
                id
            ))),
        }
    }

    async fn delete(&self, id: &ExecutionPlanId) -> AppResult<()> {
        let mut plans = self.plans.write().await;
        if plans.remove(id.as_str()).is_none() {
            return Err(AppError::NotFound(format!(
                "Execution plan not found: {}",
                id
            )));
        }
        Ok(())
    }
}
