use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;

use crate::domain::agents::{
    AgentConfig, AgentError, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse,
    AgentResult, AgenticClient, ClientCapabilities, ResponseChunk,
};
use crate::domain::repositories::ProjectRepository;

use super::mcp_policy_service::McpPolicyService;

/// Application-owned wrapper that resolves persisted/YAML MCP policy immediately
/// before every provider launch, preventing stale UI state from becoming launch state.
pub struct McpPolicyAgentClient {
    provider: AgentHarnessKind,
    inner: Arc<dyn AgenticClient>,
    policy_service: McpPolicyService,
    project_repo: Arc<dyn ProjectRepository>,
}

impl McpPolicyAgentClient {
    pub fn new(
        provider: AgentHarnessKind,
        inner: Arc<dyn AgenticClient>,
        policy_service: McpPolicyService,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self {
            provider,
            inner,
            policy_service,
            project_repo,
        }
    }

    async fn apply_policy(&self, mut config: AgentConfig) -> AgentResult<AgentConfig> {
        let project_id = match config.env.get("RALPHX_PROJECT_ID").cloned() {
            Some(project_id) => Some(project_id),
            None => self
                .project_repo
                .get_by_working_directory(&config.working_directory.to_string_lossy())
                .await
                .map_err(|error| AgentError::SpawnFailed(error.to_string()))?
                .map(|project| project.id.to_string()),
        };
        config.mcp_launch_policy = self
            .policy_service
            .resolve_launch_policy(
                self.provider,
                project_id.as_deref(),
                Some(&config.working_directory),
            )
            .await
            .map_err(|error| AgentError::SpawnFailed(error.to_string()))?;
        Ok(config)
    }
}

#[async_trait]
impl AgenticClient for McpPolicyAgentClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        self.inner
            .spawn_agent(self.apply_policy(config).await?)
            .await
    }

    async fn stop_agent(&self, handle: &AgentHandle) -> AgentResult<()> {
        self.inner.stop_agent(handle).await
    }

    async fn wait_for_completion(&self, handle: &AgentHandle) -> AgentResult<AgentOutput> {
        self.inner.wait_for_completion(handle).await
    }

    async fn send_prompt(&self, handle: &AgentHandle, prompt: &str) -> AgentResult<AgentResponse> {
        self.inner.send_prompt(handle, prompt).await
    }

    fn stream_response(
        &self,
        handle: &AgentHandle,
        prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        self.inner.stream_response(handle, prompt)
    }

    fn capabilities(&self) -> &ClientCapabilities {
        self.inner.capabilities()
    }

    async fn is_available(&self) -> AgentResult<bool> {
        self.inner.is_available().await
    }
}
