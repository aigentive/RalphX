use crate::domain::entities::{
    AgentRunId, TeamMemberId, TeamRunBinding, TeamRunBindingId, TeamSessionId,
};
use crate::domain::repositories::TeamRunBindingRepository;
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;
pub struct MemoryTeamRunBindingRepository {
    values: RwLock<HashMap<TeamRunBindingId, TeamRunBinding>>,
}
impl MemoryTeamRunBindingRepository {
    pub fn new() -> Self {
        Self {
            values: RwLock::new(HashMap::new()),
        }
    }
}
impl Default for MemoryTeamRunBindingRepository {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl TeamRunBindingRepository for MemoryTeamRunBindingRepository {
    async fn create(&self, binding: TeamRunBinding) -> AppResult<TeamRunBinding> {
        binding.validate().map_err(AppError::Validation)?;
        let mut values = self.values.write().await;
        if values
            .values()
            .any(|value| value.agent_run_id == binding.agent_run_id)
        {
            return Err(AppError::Validation("agent run already bound".to_string()));
        };
        values.insert(binding.id.clone(), binding.clone());
        Ok(binding)
    }
    async fn get_by_id(&self, id: &TeamRunBindingId) -> AppResult<Option<TeamRunBinding>> {
        Ok(self.values.read().await.get(id).cloned())
    }
    async fn get_by_agent_run_id(&self, id: &AgentRunId) -> AppResult<Option<TeamRunBinding>> {
        Ok(self
            .values
            .read()
            .await
            .values()
            .find(|value| value.agent_run_id == *id)
            .cloned())
    }
    async fn list_for_team(&self, id: &TeamSessionId) -> AppResult<Vec<TeamRunBinding>> {
        Ok(self
            .values
            .read()
            .await
            .values()
            .filter(|value| value.team_id == *id)
            .cloned()
            .collect())
    }
    async fn get_current_member_binding(
        &self,
        id: &TeamMemberId,
        generation: i64,
    ) -> AppResult<Option<TeamRunBinding>> {
        Ok(self
            .values
            .read()
            .await
            .values()
            .find(|value| value.is_current_member_authority(id, generation))
            .cloned())
    }
    async fn transition(
        &self,
        id: &TeamRunBindingId,
        expected_version: i64,
        binding: TeamRunBinding,
    ) -> AppResult<bool> {
        let mut values = self.values.write().await;
        let Some(current) = values.get(id) else {
            return Ok(false);
        };
        if current.version != expected_version {
            return Ok(false);
        };
        values.insert(id.clone(), binding);
        Ok(true)
    }
}
