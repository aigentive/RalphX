#![allow(dead_code)]

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures::Stream;
use ralphx_lib::domain::agents::{
    AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
    ClientCapabilities, ResponseChunk,
};
use ralphx_lib::domain::entities::{
    AgentWorkspacePrDescription, AgentWorkspacePrMetadataDecision, ChatConversationId,
};
use ralphx_lib::domain::repositories::AgentConversationWorkspaceRepository;
use tokio::sync::Mutex;

pub use crate::support::mock_github_service::MockGithubService;

pub struct SubmittingPlanPrAgentClient {
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    capabilities: ClientCapabilities,
    last_prompt: Mutex<Option<String>>,
}

impl SubmittingPlanPrAgentClient {
    pub fn new(workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>) -> Self {
        Self {
            workspace_repo,
            capabilities: ClientCapabilities::mock(),
            last_prompt: Mutex::new(None),
        }
    }

    async fn last_prompt(&self) -> String {
        self.last_prompt
            .lock()
            .await
            .clone()
            .expect("spawn prompt should be captured")
    }
}

#[async_trait]
impl AgenticClient for SubmittingPlanPrAgentClient {
    async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
        let role = config.role.clone();
        *self.last_prompt.lock().await = Some(config.prompt);
        Ok(AgentHandle::mock(role))
    }

    async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
        Ok(())
    }

    async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
        let prompt = self.last_prompt().await;
        let conversation_id = ChatConversationId::from_string(
            tag_value(&prompt, "conversation_id")
                .expect("plan PR describer prompt should include conversation_id"),
        );
        if prompt.contains("<publication_target kind=\"existing_pr\"") {
            self.workspace_repo
                .save_pr_metadata_decision(
                    &conversation_id,
                    AgentWorkspacePrMetadataDecision::Preserve,
                )
                .await
                .expect("test PR describer should preserve existing PR metadata");
        } else {
            self.workspace_repo
                .save_pr_description(
                    &conversation_id,
                    AgentWorkspacePrDescription::new(
                        None,
                        "## Summary\n\nDrafted by the PR-mode integration test describer"
                            .to_string(),
                    ),
                )
                .await
                .expect("test PR describer should submit a description");
        }

        Ok(AgentOutput::success("completed"))
    }

    async fn send_prompt(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> AgentResult<AgentResponse> {
        Ok(AgentResponse::default())
    }

    fn stream_response(
        &self,
        _handle: &AgentHandle,
        _prompt: &str,
    ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
        Box::pin(futures::stream::empty())
    }

    fn capabilities(&self) -> &ClientCapabilities {
        &self.capabilities
    }

    async fn is_available(&self) -> AgentResult<bool> {
        Ok(true)
    }
}

fn tag_value(prompt: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = prompt.find(&open)? + open.len();
    let end = prompt[start..].find(&close)? + start;
    Some(prompt[start..end].trim_matches('\n').to_string())
}
