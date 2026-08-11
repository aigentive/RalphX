use std::collections::{BTreeMap, HashMap};

use async_trait::async_trait;
use chrono::Utc;
use tokio::sync::RwLock;

use crate::domain::agents::{
    validate_mcp_identifier, McpOverrideState, McpPolicyOverride, McpServerKey,
};
use crate::domain::repositories::{
    mcp_policy_repository::McpPolicyRepositoryResult, McpPolicyRepository,
};

#[derive(Default)]
pub struct MemoryMcpPolicyRepository {
    rows: RwLock<HashMap<(Option<String>, McpServerKey), McpPolicyOverride>>,
}

impl MemoryMcpPolicyRepository {
    pub fn new() -> Self {
        Self::default()
    }

    async fn get(&self, project_id: Option<&str>, key: &McpServerKey) -> Option<McpPolicyOverride> {
        self.rows
            .read()
            .await
            .get(&(project_id.map(str::to_string), key.clone()))
            .cloned()
    }

    async fn mutate(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        mutation: impl FnOnce(&mut McpPolicyOverride),
    ) -> McpPolicyRepositoryResult<McpPolicyOverride> {
        let map_key = (project_id.map(str::to_string), key.clone());
        let mut rows = self.rows.write().await;
        let mut row = rows
            .get(&map_key)
            .cloned()
            .unwrap_or_else(|| McpPolicyOverride {
                project_id: project_id.map(str::to_string),
                key: key.clone(),
                server_state: McpOverrideState::Follow,
                tool_states: BTreeMap::new(),
                updated_at: Utc::now(),
            });
        mutation(&mut row);
        row.updated_at = Utc::now();
        row.validate()?;
        rows.insert(map_key, row.clone());
        Ok(row.clone())
    }
}

#[async_trait]
impl McpPolicyRepository for MemoryMcpPolicyRepository {
    async fn list_global(&self) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>> {
        let mut rows = self
            .rows
            .read()
            .await
            .values()
            .filter(|row| row.project_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.key.server_id.cmp(&right.key.server_id));
        Ok(rows)
    }

    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>> {
        let mut rows = self
            .rows
            .read()
            .await
            .values()
            .filter(|row| row.project_id.as_deref() == Some(project_id))
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|left, right| left.key.server_id.cmp(&right.key.server_id));
        Ok(rows)
    }

    async fn get_global(
        &self,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>> {
        Ok(self.get(None, key).await)
    }

    async fn get_for_project(
        &self,
        project_id: &str,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>> {
        Ok(self.get(Some(project_id), key).await)
    }

    async fn set_server_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride> {
        self.mutate(project_id, key, |row| row.server_state = state)
            .await
    }

    async fn set_tool_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride> {
        validate_mcp_identifier("tool", tool_name)?;
        self.mutate(project_id, key, |row| {
            row.tool_states.insert(tool_name.to_string(), state);
        })
        .await
    }

    async fn clear_server(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<bool> {
        let map_key = (project_id.map(str::to_string), key.clone());
        let mut rows = self.rows.write().await;
        let Some(row) = rows.get_mut(&map_key) else {
            return Ok(false);
        };
        if row.server_state == McpOverrideState::Follow {
            return Ok(false);
        }
        row.server_state = McpOverrideState::Follow;
        if row.tool_states.is_empty() {
            rows.remove(&map_key);
        }
        Ok(true)
    }

    async fn clear_tool(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
    ) -> McpPolicyRepositoryResult<bool> {
        validate_mcp_identifier("tool", tool_name)?;
        let map_key = (project_id.map(str::to_string), key.clone());
        let mut rows = self.rows.write().await;
        let Some(row) = rows.get_mut(&map_key) else {
            return Ok(false);
        };
        let removed = row.tool_states.remove(tool_name).is_some();
        if row.server_state == McpOverrideState::Follow && row.tool_states.is_empty() {
            rows.remove(&map_key);
        }
        Ok(removed)
    }
}
