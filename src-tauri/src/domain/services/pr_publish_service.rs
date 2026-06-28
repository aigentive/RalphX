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
    let footer = format!("\n\n---\n\n_Generated by [RalphX]({})_", RALPHX_REPOSITORY_URL);
    match plan_markdown {
        Some(plan) if !plan.trim().is_empty() => {
            let prefix = format!("{}\n\n", body.trim_end());
            let suffix = format!("\n\n</details>{footer}");
            let plan_header = "<details>\n<summary>View full plan</summary>\n\n";
            let full_prefix = format!("{prefix}{plan_header}");
            fit_plan_markdown_to_pr_body(&full_prefix, plan.trim(), &suffix)
        }
        _ => format!("{}{footer}", body.trim_end()),
    }
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
mod tests {
    use super::*;

    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ArtifactId, IdeationAnalysisBaseRefKind,
        IdeationSessionId, ProjectId, TaskCategory,
    };
    use crate::tests::mock_github_service::MockGithubService;

    fn agent_workspace_fixture() -> (ChatConversation, AgentConversationWorkspace) {
        let project_id = ProjectId::from_string("project-pr-publish".to_string());
        let mut conversation = ChatConversation::new_project(project_id.clone());
        conversation.title = Some("Conversation title".to_string());
        let workspace = AgentConversationWorkspace::new(
            conversation.id.clone(),
            project_id,
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some("0".repeat(40)),
            "feature/pr-description".to_string(),
            "/tmp/pr-description-worktree".to_string(),
        );
        (conversation, workspace)
    }

    #[tokio::test]
    async fn agent_workspace_publisher_creates_draft_with_generated_body() {
        let (conversation, workspace) = agent_workspace_fixture();
        let description = AgentWorkspacePrDescription::new(
            Some("Generated PR title".to_string()),
            "## Summary\n\nGenerated body".to_string(),
        );
        let github = Arc::new(MockGithubService::new());
        github.will_create_pr(42, "https://github.com/mock/repo/pull/42");
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let publisher = AgentWorkspacePrPublisher::new(&github_trait);
        let temp_dir = tempfile::tempdir().unwrap();

        let outcome = publisher
            .publish_draft_pr(temp_dir.path(), &conversation, &workspace, &description)
            .await
            .unwrap();

        assert_eq!(outcome.pr_number, 42);
        assert!(outcome.created_pr);
        assert_eq!(outcome.pr_status, "draft");
        let state = github.state();
        let (base, head, title, body_path) = state
            .last_create_draft_pr_args
            .clone()
            .expect("draft PR args should be captured");
        assert_eq!(base, "main");
        assert_eq!(head, "feature/pr-description");
        assert_eq!(title, "Generated PR title");
        assert!(body_path.contains("tmp"));
        let body = state.last_create_draft_pr_body.as_deref().unwrap();
        assert!(body.starts_with("## Summary\n\nGenerated body"));
        assert!(body.contains("_Generated by [RalphX]("));
    }

    #[tokio::test]
    async fn agent_workspace_publisher_updates_existing_pr_with_title_fallback() {
        let (mut conversation, mut workspace) = agent_workspace_fixture();
        conversation.title = Some("Untitled agent".to_string());
        workspace.publication_pr_number = Some(77);
        workspace.publication_pr_url = Some("https://github.com/mock/repo/pull/77".to_string());
        let description =
            AgentWorkspacePrDescription::new(None, "## Summary\n\nUpdated body".to_string());
        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let publisher = AgentWorkspacePrPublisher::new(&github_trait);
        let temp_dir = tempfile::tempdir().unwrap();

        let outcome = publisher
            .publish_draft_pr(temp_dir.path(), &conversation, &workspace, &description)
            .await
            .unwrap();

        assert_eq!(outcome.pr_number, 77);
        assert!(!outcome.created_pr);
        assert_eq!(outcome.pr_status, "open");
        assert_eq!(outcome.pr_url, "https://github.com/mock/repo/pull/77");
        let state = github.state();
        assert_eq!(state.update_pr_details_calls, 1);
        let (pr_number, title, body_path) = state
            .last_update_pr_details_args
            .clone()
            .expect("update PR args should be captured");
        assert_eq!(pr_number, 77);
        assert_eq!(title, "Agent conversation changes");
        assert!(body_path.contains("tmp"));
        let body = state.last_update_pr_details_body.as_deref().unwrap();
        assert!(body.starts_with("## Summary\n\nUpdated body"));
        assert!(body.contains("_Generated by [RalphX]("));
    }

    #[tokio::test]
    async fn agent_workspace_publisher_prefixes_pr_title_with_jira_key() {
        let (mut conversation, workspace) = agent_workspace_fixture();
        conversation.title = Some("rx-42: Fix composer references".to_string());
        let description = AgentWorkspacePrDescription::new(
            Some("Fix composer references".to_string()),
            "## Summary\n\nGenerated body".to_string(),
        );
        let github = Arc::new(MockGithubService::new());
        github.will_create_pr(42, "https://github.com/mock/repo/pull/42");
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let publisher = AgentWorkspacePrPublisher::new(&github_trait);
        let temp_dir = tempfile::tempdir().unwrap();

        let outcome = publisher
            .publish_draft_pr(temp_dir.path(), &conversation, &workspace, &description)
            .await
            .unwrap();

        assert_eq!(outcome.pr_number, 42);
        let state = github.state();
        let (_, _, title, _) = state
            .last_create_draft_pr_args
            .clone()
            .expect("draft PR args should be captured");
        assert_eq!(title, "RX-42: Fix composer references");
    }

    #[tokio::test]
    async fn agent_workspace_publisher_appends_plan_block_and_footer() {
        let (conversation, workspace) = agent_workspace_fixture();
        let description = AgentWorkspacePrDescription::new(
            Some("PR with plan".to_string()),
            "## Summary\n\nBody content".to_string(),
        );
        let github = Arc::new(MockGithubService::new());
        github.will_create_pr(99, "https://github.com/mock/repo/pull/99");
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let publisher = AgentWorkspacePrPublisher::new(&github_trait)
            .with_plan_markdown("## Plan\n\nStep 1\nStep 2".to_string());
        let temp_dir = tempfile::tempdir().unwrap();

        publisher
            .publish_draft_pr(temp_dir.path(), &conversation, &workspace, &description)
            .await
            .unwrap();

        let state = github.state();
        let body = state.last_create_draft_pr_body.as_deref().unwrap();
        assert!(body.starts_with("## Summary\n\nBody content"));
        assert!(body.contains("<details>\n<summary>View full plan</summary>"));
        assert!(body.contains("## Plan\n\nStep 1\nStep 2"));
        assert!(body.contains("</details>"));
        assert!(body.contains("_Generated by [RalphX]("));
    }

    #[tokio::test]
    async fn plan_pr_publisher_uses_agent_body_and_ignores_repo_template() {
        let temp_dir = tempfile::tempdir().unwrap();
        let github_dir = temp_dir.path().join(".github");
        std::fs::create_dir_all(&github_dir).unwrap();
        std::fs::write(
            github_dir.join("PULL_REQUEST_TEMPLATE.md"),
            "## Checklist\n\n- [ ] Template item\n",
        )
        .unwrap();

        let project_id = ProjectId::from_string("project-plan-pr".to_string());
        let mut project = Project::new(
            "Plan project".to_string(),
            temp_dir.path().to_string_lossy().to_string(),
        );
        project.id = project_id.clone();
        project.base_branch = Some("main".to_string());
        let task = Task::new_with_category(
            project_id.clone(),
            "Merge plan".to_string(),
            TaskCategory::PlanMerge,
        );
        let mut plan_branch = PlanBranch::new(
            ArtifactId::from_string("artifact-plan".to_string()),
            IdeationSessionId::from_string("session-plan".to_string()),
            project_id,
            "feature/plan".to_string(),
            "develop".to_string(),
        );
        plan_branch.pr_number = Some(123);

        let github = Arc::new(MockGithubService::new());
        let github_trait: Arc<dyn GithubServiceTrait> = github.clone();
        let publisher = PlanPrPublisher::new(&github_trait, None, None);
        let description = AgentWorkspacePrDescription::new(
            None,
            "## Summary\n\nAgent-written plan PR body".to_string(),
        );

        publisher
            .sync_existing_pr(
                &task,
                &project,
                &plan_branch,
                PrReviewState::Ready,
                &description,
            )
            .await
            .unwrap();

        let state = github.state();
        assert_eq!(state.update_pr_details_calls, 1);
        let (pr_number, title, _) = state
            .last_update_pr_details_args
            .clone()
            .expect("update PR args should be captured");
        assert_eq!(pr_number, 123);
        assert_eq!(title, "Merge plan");
        let body = state.last_update_pr_details_body.as_deref().unwrap();
        assert!(body.starts_with("## Summary\n\nAgent-written plan PR body"));
        assert!(!body.contains("## Checklist"));
        assert!(!body.contains("## RalphX Status"));
        assert!(!body.contains("## How To Review"));
        assert!(body.contains("_No plan artifact was available"));
        assert!(body.contains("<summary>View full plan</summary>"));
        assert!(body.contains("_Generated by [RalphX]("));
    }

    #[test]
    fn finalize_body_without_plan_appends_only_footer() {
        let body = finalize_agent_workspace_pr_body("## Summary\n\nBody", &None);
        assert!(body.starts_with("## Summary\n\nBody"));
        assert!(body.contains("_Generated by [RalphX]("));
        assert!(!body.contains("<details>"));
    }

    #[test]
    fn finalize_body_with_empty_plan_appends_only_footer() {
        let body =
            finalize_agent_workspace_pr_body("## Summary\n\nBody", &Some("  ".to_string()));
        assert!(body.starts_with("## Summary\n\nBody"));
        assert!(body.contains("_Generated by [RalphX]("));
        assert!(!body.contains("<details>"));
    }
}
