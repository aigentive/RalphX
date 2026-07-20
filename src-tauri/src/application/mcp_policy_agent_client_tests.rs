use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use tokio::sync::Mutex;

use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse, AgentResult,
    AgenticClient, ClientCapabilities, McpOverrideState, McpServerKey, ResponseChunk,
};
use crate::domain::entities::{Project, ProjectId};
use crate::domain::repositories::{McpPolicyRepository, ProjectRepository};
use crate::infrastructure::memory::{MemoryMcpPolicyRepository, MemoryProjectRepository};

use super::mcp_policy_agent_client::McpPolicyAgentClient;
use super::mcp_policy_service::McpPolicyService;

struct CapturingClient {
    capabilities: ClientCapabilities,
    captured: Mutex<Option<AgentConfig>>,
}

impl CapturingClient {
    fn new() -> Self {
        Self {
            capabilities: ClientCapabilities::mock(),
            captured: Mutex::new(None),
        }
    }

    async fn captured_config(&self) -> AgentConfig {
        self.captured
            .lock()
            .await
            .clone()
            .expect("spawn config should be captured")
    }
}

#[async_trait]
impl AgenticClient for CapturingClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        let role = config.role.clone();
        *self.captured.lock().await = Some(config);
        Ok(AgentHandle::mock(role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        Ok(AgentOutput::success("completed"))
    }

    async fn send_prompt(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> AgentResult<AgentResponse> {
        Ok(AgentResponse::new("response"))
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        Box::pin(futures::stream::iter([Ok(ResponseChunk::final_chunk(
            "streamed",
        ))]))
    }

    fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    async fn is_available(&self) -> AgentResult<bool> {
        Ok(true)
    }
}

#[tokio::test]
async fn spawn_agent_applies_project_policy_found_from_working_directory() {
    let root = tempfile::tempdir().unwrap();
    let project_root = root.path().join("project");
    std::fs::create_dir(&project_root).unwrap();
    let mut project = Project::new(
        "Test".to_string(),
        project_root.to_string_lossy().into_owned(),
    );
    project.id = ProjectId("project-1".to_string());
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(MemoryProjectRepository::with_projects(vec![project]));

    let policy_repo = Arc::new(MemoryMcpPolicyRepository::new());
    let github = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    policy_repo
        .set_tool_state(
            Some("project-1"),
            &github,
            "delete_issue",
            McpOverrideState::Disabled,
        )
        .await
        .unwrap();
    let policy_repo: Arc<dyn McpPolicyRepository> = policy_repo;
    let service = McpPolicyService::new(policy_repo, root.path().join("mcp.yaml"));
    let inner = Arc::new(CapturingClient::new());
    let client = McpPolicyAgentClient::new(
        AgentHarnessKind::Claude,
        inner.clone(),
        service,
        project_repo,
    );

    let config = AgentConfig {
        working_directory: project_root,
        ..AgentConfig::worker("run")
    };
    client.spawn_agent(config).await.unwrap();

    let captured = inner.captured_config().await;
    assert_eq!(
        captured.mcp_launch_policy.disabled_tools.get("github"),
        Some(&vec!["delete_issue".to_string()])
    );
}

#[tokio::test]
async fn spawn_agent_prefers_explicit_project_id_env_over_directory_lookup() {
    let root = tempfile::tempdir().unwrap();
    let policy_repo = Arc::new(MemoryMcpPolicyRepository::new());
    let github = McpServerKey::new(AgentHarnessKind::Claude, "github").unwrap();
    policy_repo
        .set_server_state(Some("env-project"), &github, McpOverrideState::Disabled)
        .await
        .unwrap();
    let policy_repo: Arc<dyn McpPolicyRepository> = policy_repo;
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
    let service = McpPolicyService::new(policy_repo, root.path().join("mcp.yaml"));
    let inner = Arc::new(CapturingClient::new());
    let client = McpPolicyAgentClient::new(
        AgentHarnessKind::Claude,
        inner.clone(),
        service,
        project_repo,
    );

    let mut config = AgentConfig::worker("run");
    config
        .env
        .insert("RALPHX_PROJECT_ID".to_string(), "env-project".to_string());
    client.spawn_agent(config).await.unwrap();

    let captured = inner.captured_config().await;
    assert_eq!(
        captured.mcp_launch_policy.disabled_servers,
        vec!["github".to_string()]
    );
}

#[tokio::test]
async fn delegates_non_spawn_client_operations_without_changing_results() {
    let root = tempfile::tempdir().unwrap();
    let policy_repo: Arc<dyn McpPolicyRepository> = Arc::new(MemoryMcpPolicyRepository::new());
    let project_repo: Arc<dyn ProjectRepository> = Arc::new(MemoryProjectRepository::new());
    let service = McpPolicyService::new(policy_repo, root.path().join("mcp.yaml"));
    let inner = Arc::new(CapturingClient::new());
    let client = McpPolicyAgentClient::new(
        AgentHarnessKind::Claude,
        inner.clone(),
        service,
        project_repo,
    );
    let handle = AgentHandle::mock(AgentConfig::worker("run").role);

    client.stop_agent(&handle).await.unwrap();
    assert_eq!(
        client.wait_for_completion(&handle).await.unwrap().content,
        "completed"
    );
    assert_eq!(
        client.send_prompt(&handle, "prompt").await.unwrap().content,
        "response"
    );
    let chunks = client
        .stream_response(&handle, "prompt")
        .collect::<Vec<_>>()
        .await;
    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].as_ref().unwrap().content, "streamed");
    assert!(std::ptr::eq(client.capabilities(), inner.capabilities()));
    assert!(client.is_available().await.unwrap());
}
