use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tempfile::NamedTempFile;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspacePrDescription, ArtifactContent, ChatConversation,
    PlanBranch, Project, Task,
};
use crate::domain::repositories::{ArtifactRepository, IdeationSessionRepository};
use crate::domain::services::{
    normalize_title_with_jira_key, primary_jira_key_from_title, GithubServiceTrait,
};
use crate::error::{AppError, AppResult};

#[path = "pr_publish_body.rs"]
mod body;
use body::*;

#[cfg(test)]
#[path = "pr_publish_service_tests.rs"]
mod tests;

#[async_trait]
pub trait PlanPrDescriptionDrafter: Send + Sync {
    async fn draft_plan_description(
        &self,
        project: &Project,
        plan_branch: &PlanBranch,
        review_base: &str,
        review_state: PrReviewState,
    ) -> AppResult<AgentWorkspacePrDescription>;
}

const GITHUB_PR_BODY_SOFT_LIMIT_CHARS: usize = 60_000;
const PR_BODY_TRUNCATION_NOTICE: &str =
    "\n\n_Excerpt truncated by RalphX because GitHub PR descriptions have a body size limit._";
const RALPHX_REPOSITORY_URL: &str = "https://github.com/aigentive/ralphx.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrReviewState {
    Draft,
    Ready,
}

pub struct PlanPrPublisher<'a> {
    github: &'a Arc<dyn GithubServiceTrait>,
    ideation_session_repo: Option<&'a Arc<dyn IdeationSessionRepository>>,
    artifact_repo: Option<&'a Arc<dyn ArtifactRepository>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentWorkspacePrPublishOutcome {
    pub pr_number: i64,
    pub pr_url: String,
    pub created_pr: bool,
    pub pr_status: &'static str,
}

pub struct AgentWorkspacePrPublisher<'a> {
    github: &'a Arc<dyn GithubServiceTrait>,
    plan_markdown: Option<String>,
    frozen_title: Option<String>,
}

impl<'a> AgentWorkspacePrPublisher<'a> {
    pub fn new(github: &'a Arc<dyn GithubServiceTrait>) -> Self {
        Self {
            github,
            plan_markdown: None,
            frozen_title: None,
        }
    }

    pub fn with_plan_markdown(mut self, markdown: String) -> Self {
        self.plan_markdown = Some(markdown);
        self
    }

    /// Force the immutable title supplied by a strict ticket binding.
    pub fn with_frozen_title(mut self, title: impl Into<String>) -> Self {
        self.frozen_title = Some(title.into());
        self
    }

    pub async fn update_pr_base(
        &self,
        working_dir: &Path,
        pr_number: i64,
        base: &str,
    ) -> AppResult<()> {
        self.github
            .update_pr_base(working_dir, pr_number, base)
            .await
    }

    pub async fn publish_draft_pr(
        &self,
        working_dir: &Path,
        conversation: &ChatConversation,
        workspace: &AgentConversationWorkspace,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<AgentWorkspacePrPublishOutcome> {
        let frozen_title = self
            .frozen_title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let mut title = frozen_title
            .map(str::to_string)
            .or_else(|| {
                description
                    .title
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| build_agent_workspace_pr_title(conversation));
        if frozen_title.is_none() {
            if let Some(jira_key) =
                primary_jira_key_from_title(build_agent_workspace_pr_title(conversation).as_str())
            {
                title = normalize_title_with_jira_key(&title, &jira_key);
            }
        }
        let finalized_body =
            finalize_agent_workspace_pr_body(&description.body_markdown, &self.plan_markdown);
        let body_file = write_agent_workspace_pr_body(&finalized_body)?;

        if let Some(pr_number) = workspace.publication_pr_number {
            self.github
                .update_pr_details(working_dir, pr_number, &title, body_file.path())
                .await?;
            let pr_url = workspace
                .publication_pr_url
                .clone()
                .unwrap_or_else(|| format!("#{pr_number}"));
            return Ok(AgentWorkspacePrPublishOutcome {
                pr_number,
                pr_url,
                created_pr: false,
                pr_status: "open",
            });
        }

        match self
            .github
            .create_draft_pr(
                working_dir,
                &workspace.base_ref,
                &workspace.branch_name,
                &title,
                body_file.path(),
            )
            .await
        {
            Ok((pr_number, pr_url)) => Ok(AgentWorkspacePrPublishOutcome {
                pr_number,
                pr_url,
                created_pr: true,
                pr_status: "draft",
            }),
            Err(AppError::DuplicatePr) => {
                let Some((pr_number, pr_url)) = self
                    .github
                    .find_pr_by_head_branch(working_dir, &workspace.branch_name)
                    .await?
                else {
                    return Err(AppError::DuplicatePr);
                };
                self.github
                    .update_pr_details(working_dir, pr_number, &title, body_file.path())
                    .await?;
                Ok(AgentWorkspacePrPublishOutcome {
                    pr_number,
                    pr_url,
                    created_pr: false,
                    pr_status: "open",
                })
            }
            Err(error) => Err(error),
        }
    }
}

impl<'a> PlanPrPublisher<'a> {
    pub fn new(
        github: &'a Arc<dyn GithubServiceTrait>,
        ideation_session_repo: Option<&'a Arc<dyn IdeationSessionRepository>>,
        artifact_repo: Option<&'a Arc<dyn ArtifactRepository>>,
    ) -> Self {
        Self {
            github,
            ideation_session_repo,
            artifact_repo,
        }
    }

    pub async fn create_draft_pr(
        &self,
        task: &Task,
        project: &Project,
        plan_branch: &PlanBranch,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<(i64, String)> {
        let repo_path = Path::new(&project.working_directory);
        let title = self
            .build_title(task, plan_branch, PrReviewState::Draft)
            .await;
        let body_file = self
            .write_body_file(project, plan_branch, description)
            .await?;
        let base = resolve_plan_branch_pr_base(project, plan_branch);

        self.github
            .create_draft_pr(
                repo_path,
                &base,
                &plan_branch.branch_name,
                &title,
                body_file.path(),
            )
            .await
    }

    pub async fn sync_existing_pr(
        &self,
        task: &Task,
        project: &Project,
        plan_branch: &PlanBranch,
        review_state: PrReviewState,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<()> {
        let Some(pr_number) = plan_branch.pr_number else {
            return Ok(());
        };

        let repo_path = Path::new(&project.working_directory);
        let title = self.build_title(task, plan_branch, review_state).await;
        let body_file = self
            .write_body_file(project, plan_branch, description)
            .await?;

        self.github
            .update_pr_details(repo_path, pr_number, &title, body_file.path())
            .await
    }

    async fn write_body_file(
        &self,
        project: &Project,
        plan_branch: &PlanBranch,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<NamedTempFile> {
        let body = self.build_body(project, plan_branch, description).await?;
        let body_file = NamedTempFile::new().map_err(|e| {
            AppError::Infrastructure(format!("failed to create PR body temp file: {e}"))
        })?;
        use std::io::Write as _;
        (&body_file).write_all(body.as_bytes()).map_err(|e| {
            AppError::Infrastructure(format!("failed to write PR body temp file: {e}"))
        })?;
        Ok(body_file)
    }

    async fn build_title(
        &self,
        task: &Task,
        plan_branch: &PlanBranch,
        review_state: PrReviewState,
    ) -> String {
        let display_title = self.resolve_display_title(task, plan_branch).await;
        match review_state {
            PrReviewState::Draft => format!("Plan: {}", display_title.trim()),
            PrReviewState::Ready => display_title.trim().to_string(),
        }
    }

    async fn build_body(
        &self,
        _project: &Project,
        plan_branch: &PlanBranch,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<String> {
        let plan_markdown = self
            .read_plan_artifact_markdown(plan_branch)
            .await
            .unwrap_or_else(|| {
                "_No plan artifact was available when RalphX synced this PR._".to_string()
            });

        let generated_body = description.body_markdown.trim_end();
        if generated_body.trim().is_empty() {
            return Err(AppError::Infrastructure(
                "plan PR describer returned an empty PR body".to_string(),
            ));
        }

        let footer = format!("---\n\n_Generated by [RalphX]({})_", RALPHX_REPOSITORY_URL);
        let prefix = format!(
            "{}\n\n## Plan\n\n<details>\n<summary>View full plan</summary>\n\n",
            generated_body
        );
        let suffix = format!("\n\n</details>\n\n{footer}");

        Ok(fit_plan_markdown_to_pr_body(
            &prefix,
            &plan_markdown,
            &suffix,
        ))
    }

    async fn resolve_display_title(&self, task: &Task, plan_branch: &PlanBranch) -> String {
        if let Some(repo) = self.ideation_session_repo {
            if let Ok(Some(session)) = repo.get_by_id(&plan_branch.session_id).await {
                if let Some(title) = session.title.filter(|title| !title.trim().is_empty()) {
                    return title.trim().to_string();
                }
            }
        }

        if let Some(repo) = self.artifact_repo {
            if let Ok(Some(artifact)) = repo.get_by_id(&plan_branch.plan_artifact_id).await {
                if !artifact.name.trim().is_empty() {
                    return artifact.name.trim().to_string();
                }
            }
        }

        if !task.title.trim().is_empty() {
            return task.title.trim().to_string();
        }

        plan_branch.branch_name.clone()
    }

    async fn read_plan_artifact_markdown(&self, plan_branch: &PlanBranch) -> Option<String> {
        let repo = self.artifact_repo?;
        let artifact = repo
            .get_by_id(&plan_branch.plan_artifact_id)
            .await
            .ok()
            .flatten()?;
        let raw = match artifact.content {
            ArtifactContent::Inline { text } => text,
            ArtifactContent::File { path } => tokio::fs::read_to_string(path).await.ok()?,
        };

        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }

        Some(trimmed.to_string())
    }
}

fn resolve_plan_branch_pr_base(project: &Project, plan_branch: &PlanBranch) -> String {
    plan_branch
        .base_branch_override
        .clone()
        .or_else(|| project.base_branch.clone())
        .unwrap_or_else(|| plan_branch.source_branch.clone())
}
