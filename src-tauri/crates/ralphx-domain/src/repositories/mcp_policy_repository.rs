use async_trait::async_trait;

use crate::agents::{McpOverrideState, McpPolicyOverride, McpServerKey};

pub type McpPolicyRepositoryResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[async_trait]
pub trait McpPolicyRepository: Send + Sync {
    async fn list_global(&self) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>>;
    async fn list_for_project(
        &self,
        project_id: &str,
    ) -> McpPolicyRepositoryResult<Vec<McpPolicyOverride>>;
    async fn get_global(
        &self,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>>;
    async fn get_for_project(
        &self,
        project_id: &str,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<Option<McpPolicyOverride>>;
    async fn set_server_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride>;
    async fn set_tool_state(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
        state: McpOverrideState,
    ) -> McpPolicyRepositoryResult<McpPolicyOverride>;
    async fn clear_server(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
    ) -> McpPolicyRepositoryResult<bool>;
    async fn clear_tool(
        &self,
        project_id: Option<&str>,
        key: &McpServerKey,
        tool_name: &str,
    ) -> McpPolicyRepositoryResult<bool>;
}
