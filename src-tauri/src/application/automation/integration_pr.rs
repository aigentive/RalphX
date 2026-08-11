use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::entities::{
    AgentConversationWorkspaceMode, AgentWorkspacePrDescription, Automation,
    IdeationAnalysisBaseRefKind,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, ChatConversationRepository, ProjectRepository,
};
use crate::domain::services::{AgentWorkspacePrPublisher, GithubServiceTrait};
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

#[async_trait]
pub trait AutomationIntegrationPrPublisher: Send + Sync {
    async fn ensure_integration_pr(&self, automation: &Automation) -> AppResult<()>;
}

#[cfg(test)]
#[derive(Default)]
pub struct NoopAutomationIntegrationPrPublisher;

#[cfg(test)]
#[async_trait]
impl AutomationIntegrationPrPublisher for NoopAutomationIntegrationPrPublisher {
    async fn ensure_integration_pr(&self, _automation: &Automation) -> AppResult<()> {
        Ok(())
    }
}

pub struct GithubAutomationIntegrationPrPublisher {
    github: Option<Arc<dyn GithubServiceTrait>>,
    conversation_repo: Arc<dyn ChatConversationRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    project_repo: Arc<dyn ProjectRepository>,
}

impl GithubAutomationIntegrationPrPublisher {
    pub fn new(
        github: Option<Arc<dyn GithubServiceTrait>>,
        conversation_repo: Arc<dyn ChatConversationRepository>,
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        project_repo: Arc<dyn ProjectRepository>,
    ) -> Self {
        Self {
            github,
            conversation_repo,
            workspace_repo,
            project_repo,
        }
    }
}

#[async_trait]
impl AutomationIntegrationPrPublisher for GithubAutomationIntegrationPrPublisher {
    async fn ensure_integration_pr(&self, automation: &Automation) -> AppResult<()> {
        let setup_conversation_id = automation.setup_conversation_id.as_ref().ok_or_else(|| {
            integration_pr_validation(automation, "setup conversation is missing")
        })?;
        if automation
            .base_ref_kind
            .trim()
            .parse::<IdeationAnalysisBaseRefKind>()
            != Ok(IdeationAnalysisBaseRefKind::LocalBranch)
        {
            return Err(integration_pr_validation(
                automation,
                "automation base reference is not a local branch",
            ));
        }
        let conversation = self
            .conversation_repo
            .get_by_id(setup_conversation_id)
            .await?
            .ok_or_else(|| {
                integration_pr_validation(automation, "setup conversation was not found")
            })?;
        if conversation.automation_id.as_ref() != Some(&automation.id)
            || conversation.automation_run_id.is_some()
        {
            return Err(integration_pr_validation(
                automation,
                "setup conversation ownership does not match",
            ));
        }

        let workspace = self
            .workspace_repo
            .get_by_conversation_id(setup_conversation_id)
            .await?
            .ok_or_else(|| {
                integration_pr_validation(automation, "setup workspace was not found")
            })?;
        if workspace.mode != AgentConversationWorkspaceMode::Automation
            || workspace.project_id != automation.project_id
            || workspace.branch_name != automation.base_ref
            || workspace.base_ref.trim().is_empty()
            || workspace.base_ref == workspace.branch_name
        {
            return Err(integration_pr_validation(
                automation,
                "setup workspace is not the automation integration branch",
            ));
        }
        if workspace.publication_pr_number.is_some() {
            match workspace
                .publication_pr_status
                .as_deref()
                .map(str::trim)
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("draft" | "open") => return Ok(()),
                _ => {
                    return Err(integration_pr_validation(
                        automation,
                        "existing integration pull request is not draft or open",
                    ));
                }
            }
        }

        let project = self
            .project_repo
            .get_by_id(&automation.project_id)
            .await?
            .ok_or_else(|| integration_pr_validation(automation, "project was not found"))?;
        let working_dir = validate_absolute_non_root_path(
            Path::new(&project.working_directory),
            "automation integration PR project checkout",
        )?;
        let github = self.github.as_ref().ok_or_else(|| {
            AppError::Infrastructure(
                "GitHub service is unavailable for automation integration PR publication"
                    .to_string(),
            )
        })?;
        let description = AgentWorkspacePrDescription {
            title: Some(automation.name.clone()),
            body_markdown: format!(
                "## Automation goal\n\n{}\n\nThis pull request collects the scoped changes produced by the automation on `{}`.",
                automation.goal_prompt.trim(),
                workspace.branch_name
            ),
        };
        let result = AgentWorkspacePrPublisher::new(github)
            .publish_draft_pr(&working_dir, &conversation, &workspace, &description)
            .await?;
        let outcome = match result {
            crate::domain::services::pr_publish_service::AgentWorkspacePrPublishResult::Published(
                outcome,
            ) => outcome,
            crate::domain::services::pr_publish_service::AgentWorkspacePrPublishResult::TerminalPublicationIdentity => {
                return Err(integration_pr_validation(
                    automation,
                    "workspace publication identity is terminal",
                ));
            }
        };
        self.workspace_repo
            .update_publication(
                setup_conversation_id,
                Some(outcome.pr_number),
                Some(&outcome.pr_url),
                Some(outcome.pr_status),
                Some("pushed"),
            )
            .await
    }
}

fn integration_pr_validation(automation: &Automation, detail: &str) -> AppError {
    AppError::Validation(format!(
        "automation {} integration PR cannot be published: {detail}",
        automation.id
    ))
}
