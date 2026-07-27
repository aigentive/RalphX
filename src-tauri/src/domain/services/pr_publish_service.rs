use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tempfile::NamedTempFile;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspacePrDescription, AgentWorkspacePrMetadataDecision,
    ArtifactContent, ChatConversation, PlanBranch, Project, Task,
};
use crate::domain::repositories::{ArtifactRepository, IdeationSessionRepository};
use crate::domain::services::github_generated_markdown::{
    decompose_ralphx_managed_pr_body, fit_editable_prefix_for_preserved_suffix,
    GITHUB_PR_BODY_SOFT_LIMIT_CHARS, PR_BODY_TRUNCATION_NOTICE, RALPHX_GENERATED_FOOTER,
    RALPHX_MANAGED_PR_BODY_END, RALPHX_MANAGED_PR_BODY_START,
};
use crate::domain::services::{
    normalize_title_with_jira_key, primary_jira_key_from_title, GithubServiceTrait,
};
use crate::error::{AppError, AppResult};

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
}

/// Exact existing-PR metadata patch prepared before its single remote mutation.
///
/// The body is the full recomposed GitHub value, including any preserved managed suffix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedExistingPrMetadataPatch {
    pub target_pr_number: i64,
    pub target_pr_url: Option<String>,
    pub normalized_decision: AgentWorkspacePrMetadataDecision,
    pub requested_title: Option<String>,
    pub requested_body: Option<String>,
}

impl<'a> AgentWorkspacePrPublisher<'a> {
    pub fn new(github: &'a Arc<dyn GithubServiceTrait>) -> Self {
        Self {
            github,
            plan_markdown: None,
        }
    }

    pub fn with_plan_markdown(mut self, markdown: String) -> Self {
        self.plan_markdown = Some(markdown);
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
        self.publish_draft_pr_inner(working_dir, conversation, workspace, description, true)
            .await
    }

    /// Creates a draft PR while preserving duplicate recovery for the caller.
    pub async fn publish_draft_pr_without_duplicate_recovery(
        &self,
        working_dir: &Path,
        conversation: &ChatConversation,
        workspace: &AgentConversationWorkspace,
        description: &AgentWorkspacePrDescription,
    ) -> AppResult<AgentWorkspacePrPublishOutcome> {
        self.publish_draft_pr_inner(working_dir, conversation, workspace, description, false)
            .await
    }

    async fn publish_draft_pr_inner(
        &self,
        working_dir: &Path,
        conversation: &ChatConversation,
        workspace: &AgentConversationWorkspace,
        description: &AgentWorkspacePrDescription,
        recover_duplicate: bool,
    ) -> AppResult<AgentWorkspacePrPublishOutcome> {
        let mut title = description
            .title
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| build_agent_workspace_pr_title(conversation));
        if let Some(jira_key) =
            primary_jira_key_from_title(build_agent_workspace_pr_title(conversation).as_str())
        {
            title = normalize_title_with_jira_key(&title, &jira_key);
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
            Err(AppError::DuplicatePr) if recover_duplicate => {
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
            Err(AppError::DuplicatePr) => Err(AppError::DuplicatePr),
            Err(error) => Err(error),
        }
    }

    /// Applies a deliberately partial metadata decision to a linked existing PR.
    pub async fn publish_existing_pr_metadata_decision(
        &self,
        working_dir: &Path,
        conversation: &ChatConversation,
        pr_number: i64,
        pr_url: Option<&str>,
        existing_body: Option<&str>,
        metadata_decision: &AgentWorkspacePrMetadataDecision,
    ) -> AppResult<AgentWorkspacePrPublishOutcome> {
        let prepared = self.prepare_existing_pr_metadata_patch(
            conversation,
            pr_number,
            pr_url,
            existing_body,
            metadata_decision,
        )?;
        if prepared.has_requested_fields() {
            self.mutate_prepared_existing_pr_metadata(working_dir, &prepared)
                .await?;
        }

        Ok(AgentWorkspacePrPublishOutcome {
            pr_number,
            pr_url: pr_url
                .map(str::to_string)
                .unwrap_or_else(|| format!("#{pr_number}")),
            created_pr: false,
            pr_status: "open",
        })
    }

    /// Normalizes the requested fields and recomposes a complete body without remote I/O.
    pub fn prepare_existing_pr_metadata_patch(
        &self,
        conversation: &ChatConversation,
        pr_number: i64,
        pr_url: Option<&str>,
        existing_body: Option<&str>,
        metadata_decision: &AgentWorkspacePrMetadataDecision,
    ) -> AppResult<PreparedExistingPrMetadataPatch> {
        let (requested_title, requested_body) = match metadata_decision {
            AgentWorkspacePrMetadataDecision::Preserve => (None, None),
            AgentWorkspacePrMetadataDecision::Patch {
                title,
                body_markdown,
            } => {
                let title = title.as_deref().map(|title| {
                    let mut title = title.trim().to_string();
                    if let Some(jira_key) = primary_jira_key_from_title(
                        build_agent_workspace_pr_title(conversation).as_str(),
                    ) {
                        title = normalize_title_with_jira_key(&title, &jira_key);
                    }
                    title
                });
                let body = body_markdown
                    .as_deref()
                    .map(|body| {
                        existing_body
                            .map(decompose_ralphx_managed_pr_body)
                            .and_then(|decomposition| decomposition.preserved_suffix)
                            .map(|preserved_suffix| {
                                recompose_agent_workspace_pr_body_with_preserved_suffix(
                                    body,
                                    preserved_suffix,
                                )
                            })
                            .unwrap_or_else(|| {
                                Ok(finalize_agent_workspace_pr_body(body, &self.plan_markdown))
                            })
                    })
                    .transpose()?;
                (title, body)
            }
        };
        let normalized_decision = AgentWorkspacePrMetadataDecision::patch(
            requested_title.clone(),
            requested_body.as_deref().map(str::to_string),
        )
        .unwrap_or(AgentWorkspacePrMetadataDecision::Preserve);

        Ok(PreparedExistingPrMetadataPatch {
            target_pr_number: pr_number,
            target_pr_url: pr_url.map(str::to_string),
            normalized_decision,
            requested_title,
            requested_body,
        })
    }

    /// Performs the only GitHub metadata mutation for a prepared patch.
    pub async fn mutate_prepared_existing_pr_metadata(
        &self,
        working_dir: &Path,
        prepared: &PreparedExistingPrMetadataPatch,
    ) -> AppResult<()> {
        let body_file = prepared
            .requested_body
            .as_deref()
            .map(write_agent_workspace_pr_body)
            .transpose()?;
        self.github
            .patch_pr_metadata(
                working_dir,
                prepared.target_pr_number,
                prepared.requested_title.as_deref(),
                body_file.as_ref().map(NamedTempFile::path),
            )
            .await
    }
}

impl PreparedExistingPrMetadataPatch {
    pub fn has_requested_fields(&self) -> bool {
        self.requested_title.is_some() || self.requested_body.is_some()
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

        let prefix = format!(
            "{}\n\n## Plan\n\n<details>\n<summary>View full plan</summary>\n\n",
            generated_body
        );
        let suffix = format!("\n\n</details>\n\n{RALPHX_GENERATED_FOOTER}");

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

fn build_agent_workspace_pr_title(conversation: &ChatConversation) -> String {
    conversation
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "Untitled agent")
        .map(str::to_string)
        .unwrap_or_else(|| "Agent conversation changes".to_string())
}

fn write_agent_workspace_pr_body(body: &str) -> AppResult<NamedTempFile> {
    let body_file = NamedTempFile::new().map_err(|e| {
        AppError::Infrastructure(format!("failed to create PR body temp file: {e}"))
    })?;
    use std::io::Write as _;
    (&body_file)
        .write_all(body.as_bytes())
        .map_err(|e| AppError::Infrastructure(format!("failed to write PR body temp file: {e}")))?;
    Ok(body_file)
}

fn finalize_agent_workspace_pr_body(body: &str, plan_markdown: &Option<String>) -> String {
    match plan_markdown {
        Some(plan) if !plan.trim().is_empty() => {
            let editable_prefix = body.trim_end();
            let managed_prefix = format!("\n\n{RALPHX_MANAGED_PR_BODY_START}\n");
            let suffix = format!(
                "\n\n</details>\n\n{RALPHX_GENERATED_FOOTER}\n{RALPHX_MANAGED_PR_BODY_END}"
            );
            let plan_header = "<details>\n<summary>View full plan</summary>\n\n";
            let full_body = format!(
                "{editable_prefix}{managed_prefix}{plan_header}{}{suffix}",
                plan.trim()
            );
            if char_count(&full_body) <= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
                return full_body;
            }
            let fixed_chars = char_count(&managed_prefix)
                + char_count(plan_header)
                + char_count(&suffix)
                + char_count(PR_BODY_TRUNCATION_NOTICE);
            let available_content_chars =
                GITHUB_PR_BODY_SOFT_LIMIT_CHARS.saturating_sub(fixed_chars);
            let editable = truncate_chars(editable_prefix, available_content_chars);
            let remaining_plan_chars =
                available_content_chars.saturating_sub(char_count(editable.trim_end()));
            let truncated_plan = truncate_chars(plan.trim(), remaining_plan_chars);
            format!(
                "{}{managed_prefix}{plan_header}{}{PR_BODY_TRUNCATION_NOTICE}{suffix}",
                editable.trim_end(),
                truncated_plan.trim_end()
            )
        }
        _ => {
            let preserved_suffix = format!(
                "\n\n{RALPHX_MANAGED_PR_BODY_START}\n{RALPHX_GENERATED_FOOTER}\n\
                 {RALPHX_MANAGED_PR_BODY_END}"
            );
            let editable_prefix = body
                .trim_end()
                .strip_suffix(RALPHX_GENERATED_FOOTER)
                .map(str::trim_end)
                .unwrap_or(body);
            recompose_agent_workspace_pr_body_with_preserved_suffix(
                editable_prefix,
                &preserved_suffix,
            )
            .unwrap_or(preserved_suffix)
        }
    }
}

fn recompose_agent_workspace_pr_body_with_preserved_suffix(
    editable_prefix: &str,
    preserved_suffix: &str,
) -> AppResult<String> {
    let editable_prefix = fit_editable_prefix_for_preserved_suffix(
        editable_prefix,
        preserved_suffix,
    )
    .ok_or_else(|| {
        AppError::Validation(
            "the preserved PR body suffix leaves no room for an editable description".to_string(),
        )
    })?;
    Ok(format!("{editable_prefix}{preserved_suffix}"))
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn fit_plan_markdown_to_pr_body(prefix: &str, plan_markdown: &str, suffix: &str) -> String {
    let full_body = format!("{prefix}{plan_markdown}{suffix}");
    if char_count(&full_body) <= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
        return full_body;
    }

    let fixed_chars =
        char_count(prefix) + char_count(suffix) + char_count(PR_BODY_TRUNCATION_NOTICE);
    if fixed_chars >= GITHUB_PR_BODY_SOFT_LIMIT_CHARS {
        return truncate_chars(&full_body, GITHUB_PR_BODY_SOFT_LIMIT_CHARS);
    }

    let available_plan_chars = GITHUB_PR_BODY_SOFT_LIMIT_CHARS - fixed_chars;
    let truncated_plan = truncate_chars(plan_markdown, available_plan_chars);
    format!(
        "{prefix}{}{}{suffix}",
        truncated_plan.trim_end(),
        PR_BODY_TRUNCATION_NOTICE
    )
}

#[cfg(test)]
#[path = "pr_publish_service_tests.rs"]
mod tests;
