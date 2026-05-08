use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::git_service::git_cmd;
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::{AppState, GitService};
use crate::domain::agents::{AgentConfig, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspacePrDescription, ChatConversation, Project,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;

pub const DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE: &str =
    include_str!("../../../.github/PULL_REQUEST_TEMPLATE.md");

const MAX_AGENT_WORKSPACE_PR_BODY_CHARS: usize = 60_000;
const MAX_PATCH_EXCERPT_CHARS: usize = 42_000;
const MAX_CONVERSATION_CONTEXT_CHARS: usize = 12_000;
const MAX_NAME_STATUS_CHARS: usize = 16_000;
const MAX_STAT_CHARS: usize = 8_000;
const MAX_MESSAGE_CHARS: usize = 1_600;
const MAX_CONTEXT_MESSAGES: usize = 12;
const MAX_COMMIT_SUMMARIES: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PullRequestTemplateContext {
    source: &'static str,
    content: String,
}

pub fn validate_agent_workspace_pr_description_body(body: &str) -> AppResult<()> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "PR description body cannot be empty".to_string(),
        ));
    }

    let chars = trimmed.chars().count();
    if chars > MAX_AGENT_WORKSPACE_PR_BODY_CHARS {
        return Err(AppError::Validation(format!(
            "PR description body is too long ({chars} characters; maximum is {MAX_AGENT_WORKSPACE_PR_BODY_CHARS})"
        )));
    }

    Ok(())
}

pub async fn draft_agent_workspace_pr_description(
    state: &AppState,
    conversation: &ChatConversation,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    workspace_path: &Path,
    review_base: &str,
) -> AppResult<AgentWorkspacePrDescription> {
    state
        .agent_conversation_workspace_repo
        .clear_pr_description(&workspace.conversation_id)
        .await?;

    let template = read_pull_request_template_context(project, workspace_path).await;
    let diff_stats =
        GitService::get_diff_stats_between(workspace_path, review_base, "HEAD").await?;
    let commits = GitService::get_commits_between(workspace_path, review_base, "HEAD").await?;
    let name_status = run_git_text(
        workspace_path,
        &[
            "diff",
            "--find-renames",
            "--name-status",
            &format!("{review_base}..HEAD"),
        ],
    )
    .await?;
    let diff_stat = run_git_text(
        workspace_path,
        &[
            "diff",
            "--find-renames",
            "--stat",
            &format!("{review_base}..HEAD"),
        ],
    )
    .await?;
    let patch_excerpt = run_git_text(
        workspace_path,
        &[
            "diff",
            "--find-renames",
            "--minimal",
            "--no-ext-diff",
            &format!("{review_base}..HEAD"),
        ],
    )
    .await?;
    let conversation_context = build_conversation_context(state, conversation).await?;
    let prompt = build_pr_describer_prompt(PrDescriberPromptContext {
        conversation,
        project,
        workspace,
        effective_cwd: workspace_path,
        review_base,
        template: &template,
        commits: &commits,
        diff_stats: &diff_stats,
        name_status: &name_status,
        diff_stat: &diff_stat,
        patch_excerpt: &patch_excerpt,
        conversation_context: &conversation_context,
    });

    let runtime = state.resolve_pr_describer_runtime(conversation).await?;
    let agent_client = Arc::clone(&runtime.client);
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        helper_harness,
        agent_names::AGENT_PR_DESCRIBER,
        PathBuf::from(&project.working_directory),
    );

    let output = agent_client
        .spawn_agent(AgentConfig {
            role: AgentRole::Custom(bootstrap.agent_role.clone()),
            prompt,
            working_directory: bootstrap.working_directory,
            plugin_dir: Some(bootstrap.plugin_dir),
            agent: Some(bootstrap.agent_name),
            model: runtime.model,
            harness: runtime.harness,
            logical_effort: runtime.logical_effort,
            approval_policy: runtime.approval_policy,
            sandbox_mode: runtime.sandbox_mode,
            max_tokens: None,
            timeout_secs: Some(120),
            env: bootstrap.env,
        })
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("failed to spawn PR describer agent: {error}"))
        })?;

    let output = agent_client
        .wait_for_completion(&output)
        .await
        .map_err(|error| AppError::Infrastructure(format!("PR describer agent failed: {error}")))?;
    if !output.success {
        return Err(AppError::Infrastructure(format!(
            "PR describer agent exited unsuccessfully: {}",
            output.content.trim()
        )));
    }

    let Some(description) = state
        .agent_conversation_workspace_repo
        .get_pr_description(&workspace.conversation_id)
        .await?
    else {
        return Err(AppError::Infrastructure(
            "PR describer agent completed without submitting a PR description".to_string(),
        ));
    };

    validate_agent_workspace_pr_description_body(&description.body_markdown)?;
    Ok(AgentWorkspacePrDescription::new(
        description.title,
        description.body_markdown,
    ))
}

async fn read_pull_request_template_context(
    project: &Project,
    workspace_path: &Path,
) -> PullRequestTemplateContext {
    if let Some(content) = read_template(workspace_path).await {
        return PullRequestTemplateContext {
            source: "workspace",
            content,
        };
    }

    let project_path = PathBuf::from(&project.working_directory);
    if project_path != workspace_path {
        if let Some(content) = read_template(&project_path).await {
            return PullRequestTemplateContext {
                source: "project",
                content,
            };
        }
    }

    PullRequestTemplateContext {
        source: "ralphx_fallback",
        content: DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.trim().to_string(),
    }
}

async fn read_template(repo_path: &Path) -> Option<String> {
    let template_path = repo_path.join(".github").join("PULL_REQUEST_TEMPLATE.md");
    match tokio::fs::read_to_string(template_path).await {
        Ok(content) if !content.trim().is_empty() => Some(content.trim().to_string()),
        _ => None,
    }
}

async fn run_git_text(repo: &Path, args: &[&str]) -> AppResult<String> {
    let output = git_cmd::run(args, repo).await?;
    if !output.status.success() {
        return Err(AppError::GitOperation(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn build_conversation_context(
    state: &AppState,
    conversation: &ChatConversation,
) -> AppResult<String> {
    let messages = state
        .chat_message_repo
        .get_by_conversation(&conversation.id)
        .await?;
    let start = messages.len().saturating_sub(MAX_CONTEXT_MESSAGES);
    let mut context = String::new();
    for message in messages.iter().skip(start) {
        let content = truncate_chars(message.content.trim(), MAX_MESSAGE_CHARS);
        if content.is_empty() {
            continue;
        }
        context.push_str(&format!(
            "[{} at {}]\n{}\n\n",
            message.role, message.created_at, content
        ));
        if context.chars().count() >= MAX_CONVERSATION_CONTEXT_CHARS {
            return Ok(truncate_chars(&context, MAX_CONVERSATION_CONTEXT_CHARS));
        }
    }
    Ok(context)
}

struct PrDescriberPromptContext<'a> {
    conversation: &'a ChatConversation,
    project: &'a Project,
    workspace: &'a AgentConversationWorkspace,
    effective_cwd: &'a Path,
    review_base: &'a str,
    template: &'a PullRequestTemplateContext,
    commits: &'a [crate::application::git_service::CommitInfo],
    diff_stats: &'a crate::application::git_service::DiffStats,
    name_status: &'a str,
    diff_stat: &'a str,
    patch_excerpt: &'a str,
    conversation_context: &'a str,
}

fn build_pr_describer_prompt(ctx: PrDescriberPromptContext<'_>) -> String {
    let commit_summaries = format_commit_summaries(ctx.commits);
    let changed_files = format_changed_files(ctx.diff_stats);
    let diff_summary = format!(
        "{} files changed, {} insertions, {} deletions",
        ctx.diff_stats.files_changed, ctx.diff_stats.insertions, ctx.diff_stats.deletions
    );

    format!(
        "<instructions>\n\
         Write a reviewer-focused pull request description for this agent conversation workspace publish.\n\
         Follow the supplied pull request template structure exactly. If a section is not applicable, keep the heading and say so briefly.\n\
         Use only the supplied conversation, commit, and diff context. Do not invent validation, test results, product impact, or user-visible behavior.\n\
         Do not include command transcripts, local validation logs, or agent progress diaries.\n\
         Do not mention bounded input limits, excerpt truncation, omitted prompt context, or ask reviewers to compensate for missing helper input.\n\
         If the supplied code context is genuinely ambiguous, name only the product or technical risk you can infer.\n\
         If validation evidence is absent, omit validation claims instead of treating absent validation as a risk.\n\
         Call submit_agent_workspace_pr_description exactly once with conversation_id and body_markdown. Include title only if you can produce a better reviewer-facing PR title than the existing conversation title.\n\
         Do not inspect files, run shell commands, modify files, delegate, or perform any action other than submitting the PR description.\n\
         </instructions>\n\
         <data>\n\
         <conversation_id>{conversation_id}</conversation_id>\n\
         <conversation_title>{conversation_title}</conversation_title>\n\
         <project_name>{project_name}</project_name>\n\
         <registered_project_cwd>{project_cwd}</registered_project_cwd>\n\
         <effective_workspace_cwd>{effective_cwd}</effective_workspace_cwd>\n\
         <base_ref>{base_ref}</base_ref>\n\
         <base_commit>{base_commit}</base_commit>\n\
         <branch_name>{branch_name}</branch_name>\n\
         <review_base>{review_base}</review_base>\n\
         <template source=\"{template_source}\">\n{template}\n</template>\n\
         <diff_summary>{diff_summary}</diff_summary>\n\
         <commit_summaries>\n{commit_summaries}\n</commit_summaries>\n\
         <changed_files>\n{changed_files}\n</changed_files>\n\
         <name_status>\n{name_status}\n</name_status>\n\
         <diff_stat>\n{diff_stat}\n</diff_stat>\n\
         <patch_excerpt>\n{patch_excerpt}\n</patch_excerpt>\n\
         <conversation_context>\n{conversation_context}\n</conversation_context>\n\
         </data>",
        conversation_id = ctx.workspace.conversation_id,
        conversation_title = escape_xml_text(ctx.conversation.title.as_deref().unwrap_or("")),
        project_name = escape_xml_text(&ctx.project.name),
        project_cwd = escape_xml_text(&ctx.project.working_directory),
        effective_cwd = escape_xml_text(&ctx.effective_cwd.display().to_string()),
        base_ref = escape_xml_text(&ctx.workspace.base_ref),
        base_commit = escape_xml_text(ctx.workspace.base_commit.as_deref().unwrap_or("")),
        branch_name = escape_xml_text(&ctx.workspace.branch_name),
        review_base = escape_xml_text(ctx.review_base),
        template_source = ctx.template.source,
        template = escape_xml_text(&ctx.template.content),
        diff_summary = escape_xml_text(&diff_summary),
        commit_summaries = escape_xml_text(&commit_summaries),
        changed_files = escape_xml_text(&changed_files),
        name_status = escape_xml_text(&truncate_chars(ctx.name_status, MAX_NAME_STATUS_CHARS)),
        diff_stat = escape_xml_text(&truncate_chars(ctx.diff_stat, MAX_STAT_CHARS)),
        patch_excerpt = escape_xml_text(&truncate_chars(ctx.patch_excerpt, MAX_PATCH_EXCERPT_CHARS)),
        conversation_context = escape_xml_text(ctx.conversation_context),
    )
}

fn format_commit_summaries(commits: &[crate::application::git_service::CommitInfo]) -> String {
    if commits.is_empty() {
        return "No commit summaries were available.".to_string();
    }

    let lines = commits
        .iter()
        .take(MAX_COMMIT_SUMMARIES)
        .map(|commit| {
            format!(
                "- {} {} ({}, {})",
                commit.short_sha, commit.message, commit.author, commit.timestamp
            )
        })
        .collect::<Vec<_>>();
    lines.join("\n")
}

fn format_changed_files(diff_stats: &crate::application::git_service::DiffStats) -> String {
    if diff_stats.changed_files.is_empty() {
        return "No changed files were reported by git diff.".to_string();
    }
    diff_stats
        .changed_files
        .iter()
        .map(|file| format!("- {file}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::process::Command;

    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use futures::{stream, Stream};
    use tempfile::TempDir;

    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse, AgentResult,
        AgentRole, AgenticClient, ClientCapabilities, ResponseChunk,
    };
    use crate::domain::entities::{
        AgentConversationWorkspaceMode, ChatMessage, IdeationAnalysisBaseRefKind, MessageRole,
    };
    use crate::domain::repositories::AgentConversationWorkspaceRepository;

    struct SubmittingPrDescriptionClient {
        repo: Arc<dyn AgentConversationWorkspaceRepository>,
        conversation_id: crate::domain::entities::ChatConversationId,
        title: Option<String>,
        body_markdown: String,
        output: AgentOutput,
        capabilities: ClientCapabilities,
        spawned_configs: tokio::sync::Mutex<Vec<AgentConfig>>,
    }

    impl SubmittingPrDescriptionClient {
        fn success(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: crate::domain::entities::ChatConversationId,
            title: Option<String>,
            body_markdown: impl Into<String>,
        ) -> Self {
            Self {
                repo,
                conversation_id,
                title,
                body_markdown: body_markdown.into(),
                output: AgentOutput::success("submitted"),
                capabilities: ClientCapabilities::mock(),
                spawned_configs: tokio::sync::Mutex::new(Vec::new()),
            }
        }

        fn failed(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: crate::domain::entities::ChatConversationId,
        ) -> Self {
            Self {
                repo,
                conversation_id,
                title: None,
                body_markdown: String::new(),
                output: AgentOutput::failed("agent failed", 1),
                capabilities: ClientCapabilities::mock(),
                spawned_configs: tokio::sync::Mutex::new(Vec::new()),
            }
        }

        async fn spawned_configs(&self) -> Vec<AgentConfig> {
            self.spawned_configs.lock().await.clone()
        }
    }

    #[async_trait]
    impl AgenticClient for SubmittingPrDescriptionClient {
        async fn spawn_agent(&self, config: AgentConfig) -> AgentResult<AgentHandle> {
            self.spawned_configs.lock().await.push(config.clone());
            Ok(AgentHandle::mock(config.role))
        }

        async fn stop_agent(&self, _handle: &AgentHandle) -> AgentResult<()> {
            Ok(())
        }

        async fn wait_for_completion(&self, _handle: &AgentHandle) -> AgentResult<AgentOutput> {
            if self.output.success {
                self.repo
                    .save_pr_description(
                        &self.conversation_id,
                        AgentWorkspacePrDescription::new(
                            self.title.clone(),
                            self.body_markdown.clone(),
                        ),
                    )
                    .await
                    .expect("test PR description should save");
            }
            Ok(self.output.clone())
        }

        async fn send_prompt(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> AgentResult<AgentResponse> {
            Ok(AgentResponse::new(""))
        }

        fn stream_response(
            &self,
            _handle: &AgentHandle,
            _prompt: &str,
        ) -> Pin<Box<dyn Stream<Item = AgentResult<ResponseChunk>> + Send>> {
            Box::pin(stream::empty())
        }

        fn capabilities(&self) -> &ClientCapabilities {
            &self.capabilities
        }

        async fn is_available(&self) -> AgentResult<bool> {
            Ok(true)
        }
    }

    fn run_git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git command should run");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn create_reviewable_repo() -> (TempDir, PathBuf, String) {
        let temp_dir = TempDir::new().expect("temp repo should be created");
        let repo = temp_dir.path().to_path_buf();
        run_git(&repo, &["init", "-b", "main"]);
        run_git(&repo, &["config", "user.email", "test@example.com"]);
        run_git(&repo, &["config", "user.name", "Test User"]);

        std::fs::create_dir_all(repo.join(".github")).unwrap();
        std::fs::write(
            repo.join(".github").join("PULL_REQUEST_TEMPLATE.md"),
            "## Summary\n\n## User Impact\n\n## Risks / Follow-Ups\n",
        )
        .unwrap();
        std::fs::write(repo.join("README.md"), "initial\n").unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Initial commit"]);
        let base = run_git(&repo, &["rev-parse", "HEAD"]);

        run_git(&repo, &["checkout", "-b", "feature/pr-description"]);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            repo.join("src").join("lib.rs"),
            "pub fn answer() -> u8 { 42 }\n",
        )
        .unwrap();
        run_git(&repo, &["add", "."]);
        run_git(&repo, &["commit", "-m", "Add PR description helper"]);

        (temp_dir, repo, base)
    }

    fn project_for(repo: &Path) -> Project {
        Project::new("Example <Project>".to_string(), repo.display().to_string())
    }

    fn conversation_for(project: &Project) -> ChatConversation {
        let mut conversation = ChatConversation::new_project(project.id.clone());
        conversation.title = Some("Improve PR descriptions & publishing".to_string());
        conversation
    }

    fn workspace_for(
        conversation: &ChatConversation,
        project: &Project,
        repo: &Path,
        base: &str,
    ) -> AgentConversationWorkspace {
        AgentConversationWorkspace::new(
            conversation.id.clone(),
            project.id.clone(),
            AgentConversationWorkspaceMode::Edit,
            IdeationAnalysisBaseRefKind::ProjectDefault,
            "main".to_string(),
            Some("main".to_string()),
            Some(base.to_string()),
            "feature/pr-description".to_string(),
            repo.display().to_string(),
        )
    }

    fn message_for_conversation(
        conversation: &ChatConversation,
        role: MessageRole,
        content: impl Into<String>,
        offset_seconds: i64,
    ) -> ChatMessage {
        let mut message =
            ChatMessage::user_in_project(crate::domain::entities::ProjectId::new(), content.into());
        message.conversation_id = Some(conversation.id.clone());
        message.project_id = None;
        message.role = role;
        message.created_at = Utc::now() + Duration::seconds(offset_seconds);
        message
    }

    #[test]
    fn validation_rejects_empty_body() {
        let error = validate_agent_workspace_pr_description_body("  ").unwrap_err();
        assert!(error.to_string().contains("cannot be empty"));
    }

    #[test]
    fn validation_rejects_overlong_body_and_accepts_trimmed_content() {
        validate_agent_workspace_pr_description_body("  ## Summary\nUseful body\n").unwrap();

        let body = "x".repeat(MAX_AGENT_WORKSPACE_PR_BODY_CHARS + 1);
        let error = validate_agent_workspace_pr_description_body(&body).unwrap_err();
        assert!(error.to_string().contains("too long"));
    }

    #[test]
    fn fallback_template_has_expected_sections() {
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## Summary"));
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## User Impact"));
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## Risks / Follow-Ups"));
    }

    #[tokio::test]
    async fn template_context_prefers_workspace_then_project_then_fallback() {
        let project_dir = TempDir::new().unwrap();
        let workspace_dir = TempDir::new().unwrap();
        let mut project = project_for(project_dir.path());
        project.working_directory = project_dir.path().display().to_string();

        std::fs::create_dir_all(project_dir.path().join(".github")).unwrap();
        std::fs::create_dir_all(workspace_dir.path().join(".github")).unwrap();
        std::fs::write(
            project_dir
                .path()
                .join(".github")
                .join("PULL_REQUEST_TEMPLATE.md"),
            "## Project Template\n",
        )
        .unwrap();
        std::fs::write(
            workspace_dir
                .path()
                .join(".github")
                .join("PULL_REQUEST_TEMPLATE.md"),
            "## Workspace Template\n",
        )
        .unwrap();

        let context = read_pull_request_template_context(&project, workspace_dir.path()).await;
        assert_eq!(context.source, "workspace");
        assert_eq!(context.content, "## Workspace Template");

        std::fs::remove_file(
            workspace_dir
                .path()
                .join(".github")
                .join("PULL_REQUEST_TEMPLATE.md"),
        )
        .unwrap();
        let context = read_pull_request_template_context(&project, workspace_dir.path()).await;
        assert_eq!(context.source, "project");
        assert_eq!(context.content, "## Project Template");

        std::fs::write(
            project_dir
                .path()
                .join(".github")
                .join("PULL_REQUEST_TEMPLATE.md"),
            "   \n",
        )
        .unwrap();
        let context = read_pull_request_template_context(&project, workspace_dir.path()).await;
        assert_eq!(context.source, "ralphx_fallback");
        assert!(context.content.contains("## User Impact"));
    }

    #[tokio::test]
    async fn run_git_text_returns_stdout_and_nonzero_errors() {
        let (_temp_dir, repo, _base) = create_reviewable_repo();

        let output = run_git_text(&repo, &["status", "--short"]).await.unwrap();
        assert!(output.trim().is_empty());

        let error = run_git_text(&repo, &["definitely-not-a-command"])
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("git definitely-not-a-command failed"));
    }

    #[tokio::test]
    async fn conversation_context_uses_recent_non_empty_messages_and_truncates() {
        let state = AppState::new_test();
        let project = Project::new("Project".to_string(), "/tmp/project".to_string());
        let conversation = conversation_for(&project);

        for index in 0..14 {
            let content = if index == 3 {
                "   ".to_string()
            } else {
                format!("message-{index} {}", "x".repeat(MAX_MESSAGE_CHARS + 20))
            };
            let message = message_for_conversation(
                &conversation,
                if index % 2 == 0 {
                    MessageRole::User
                } else {
                    MessageRole::Orchestrator
                },
                content,
                index,
            );
            state.chat_message_repo.create(message).await.unwrap();
        }

        let context = build_conversation_context(&state, &conversation)
            .await
            .unwrap();

        assert!(!context.contains("message-0 "));
        assert!(!context.contains("message-1 "));
        assert!(context.contains("[user at "));
        assert!(context.contains("[orchestrator at "));
        assert!(
            !context.contains("truncated by RalphX"),
            "conversation context should not teach the PR describer to mention prompt truncation"
        );
        assert!(context.chars().count() <= MAX_CONVERSATION_CONTEXT_CHARS);
    }

    #[test]
    fn prompt_context_formats_escaped_bounded_diff_data() {
        let (_temp_dir, repo, base) = create_reviewable_repo();
        let mut project = project_for(&repo);
        project.name = "Project <A&B>".to_string();
        let conversation = conversation_for(&project);
        let workspace = workspace_for(&conversation, &project, &repo, &base);
        let commits = (0..(MAX_COMMIT_SUMMARIES + 2))
            .map(|index| crate::application::git_service::CommitInfo {
                sha: format!("{index:040}"),
                short_sha: format!("{index:07}"),
                message: format!("Commit <{index}> & details"),
                author: "A&B".to_string(),
                timestamp: "2026-05-06T00:00:00Z".to_string(),
            })
            .collect::<Vec<_>>();
        let diff_stats = crate::application::git_service::DiffStats {
            files_changed: 2,
            insertions: 10,
            deletions: 3,
            changed_files: vec!["src/lib.rs".to_string(), "README.md".to_string()],
        };
        let prompt = build_pr_describer_prompt(PrDescriberPromptContext {
            conversation: &conversation,
            project: &project,
            workspace: &workspace,
            effective_cwd: &repo,
            review_base: "origin/main & HEAD",
            template: &PullRequestTemplateContext {
                source: "workspace",
                content: "## Summary\nUse <template> & context".to_string(),
            },
            commits: &commits,
            diff_stats: &diff_stats,
            name_status: &"M\tfile\n".repeat(MAX_NAME_STATUS_CHARS + 1),
            diff_stat: &" stat\n".repeat(MAX_STAT_CHARS + 1),
            patch_excerpt: &"diff --git a/file b/file\n".repeat(MAX_PATCH_EXCERPT_CHARS + 1),
            conversation_context: "[user] use <context> & facts",
        });

        assert!(prompt.contains("<project_name>Project &lt;A&amp;B&gt;</project_name>"));
        assert!(prompt.contains("<template source=\"workspace\">"));
        assert!(prompt.contains("Use &lt;template&gt; &amp; context"));
        assert!(prompt.contains("2 files changed, 10 insertions, 3 deletions"));
        assert!(prompt.contains("- src/lib.rs"));
        assert!(
            !prompt.contains("omitted by RalphX"),
            "bounded prompt data should not expose internal omission mechanics"
        );
        assert!(
            !prompt.contains("truncated by RalphX"),
            "bounded prompt data should not teach the PR describer to surface RalphX truncation mechanics"
        );
        assert!(
            prompt.contains("Do not mention bounded input limits"),
            "prompt should explicitly keep bounded-context mechanics out of reviewer-facing PR bodies"
        );
        assert!(prompt.contains("use &lt;context&gt; &amp; facts"));
    }

    #[test]
    fn empty_commit_and_file_context_has_explicit_placeholders() {
        let diff_stats = crate::application::git_service::DiffStats {
            files_changed: 0,
            insertions: 0,
            deletions: 0,
            changed_files: Vec::new(),
        };

        assert_eq!(
            format_commit_summaries(&[]),
            "No commit summaries were available."
        );
        assert_eq!(
            format_changed_files(&diff_stats),
            "No changed files were reported by git diff."
        );
        assert_eq!(
            escape_xml_text("a < b && c > d"),
            "a &lt; b &amp;&amp; c &gt; d"
        );
    }

    #[tokio::test]
    async fn draft_pr_description_collects_git_context_and_uses_submitted_body() {
        let (_temp_dir, repo, base) = create_reviewable_repo();
        let active_project_dir = tempfile::tempdir().unwrap();
        let project = project_for(active_project_dir.path());
        let conversation = conversation_for(&project);
        let workspace = workspace_for(&conversation, &project, &repo, &base);
        let state = AppState::new_test();
        state
            .chat_message_repo
            .create(message_for_conversation(
                &conversation,
                MessageRole::User,
                "Please prepare the publishable PR description.",
                1,
            ))
            .await
            .unwrap();
        state
            .agent_conversation_workspace_repo
            .save_pr_description(
                &conversation.id,
                AgentWorkspacePrDescription::new(
                    Some("stale title".to_string()),
                    "stale body".to_string(),
                ),
            )
            .await
            .unwrap();

        let client = Arc::new(SubmittingPrDescriptionClient::success(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation.id.clone(),
            Some("Reviewer-focused PR title".to_string()),
            "## Summary\n\nReal body from utility agent.",
        ));
        let state = state.with_agent_client(client.clone());

        let description = draft_agent_workspace_pr_description(
            &state,
            &conversation,
            &project,
            &workspace,
            &repo,
            &base,
        )
        .await
        .unwrap();

        assert_eq!(
            description.title.as_deref(),
            Some("Reviewer-focused PR title")
        );
        assert_eq!(
            description.body_markdown,
            "## Summary\n\nReal body from utility agent."
        );

        let configs = client.spawned_configs().await;
        assert_eq!(configs.len(), 1);
        let config = &configs[0];
        assert_eq!(
            config.role,
            AgentRole::Custom("ralphx-utility-pr-describer".to_string())
        );
        assert_eq!(config.working_directory, active_project_dir.path());
        assert_eq!(
            config.agent.as_deref(),
            Some(agent_names::AGENT_PR_DESCRIBER)
        );
        assert_eq!(config.timeout_secs, Some(120));
        assert!(config
            .prompt
            .contains("submit_agent_workspace_pr_description"));
        assert!(config.prompt.contains(&format!(
            "<registered_project_cwd>{}</registered_project_cwd>",
            escape_xml_text(&active_project_dir.path().display().to_string())
        )));
        assert!(config.prompt.contains(&format!(
            "<effective_workspace_cwd>{}</effective_workspace_cwd>",
            escape_xml_text(&repo.display().to_string())
        )));
        assert!(config.prompt.contains("src/lib.rs"));
        assert!(config
            .prompt
            .contains("Please prepare the publishable PR description."));
    }

    #[tokio::test]
    async fn draft_pr_description_uses_conversation_harness_client_when_available() {
        let (_temp_dir, repo, base) = create_reviewable_repo();
        let project = project_for(&repo);
        let mut conversation = conversation_for(&project);
        conversation.provider_harness = Some(AgentHarnessKind::Codex);
        let workspace = workspace_for(&conversation, &project, &repo, &base);
        let state = AppState::new_test();

        let default_client = Arc::new(SubmittingPrDescriptionClient::failed(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation.id.clone(),
        ));
        let codex_client = Arc::new(SubmittingPrDescriptionClient::success(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation.id.clone(),
            Some("Codex PR title".to_string()),
            "## Summary\n\nCodex-generated body.",
        ));
        let state = state
            .with_agent_client(default_client.clone())
            .with_harness_agent_client(AgentHarnessKind::Codex, codex_client.clone());

        let description = draft_agent_workspace_pr_description(
            &state,
            &conversation,
            &project,
            &workspace,
            &repo,
            &base,
        )
        .await
        .unwrap();

        assert_eq!(description.title.as_deref(), Some("Codex PR title"));
        assert_eq!(
            description.body_markdown,
            "## Summary\n\nCodex-generated body."
        );
        assert!(
            default_client.spawned_configs().await.is_empty(),
            "default helper client should not be used for Codex-owned conversations"
        );

        let configs = codex_client.spawned_configs().await;
        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].harness, Some(AgentHarnessKind::Codex));
        assert_eq!(configs[0].approval_policy.as_deref(), Some("never"));
        assert_eq!(
            configs[0].sandbox_mode.as_deref(),
            Some("danger-full-access")
        );
        assert_eq!(
            configs[0].agent.as_deref(),
            Some(agent_names::AGENT_PR_DESCRIBER)
        );
    }

    #[tokio::test]
    async fn draft_pr_description_surfaces_unsuccessful_agent_output() {
        let (_temp_dir, repo, base) = create_reviewable_repo();
        let project = project_for(&repo);
        let conversation = conversation_for(&project);
        let workspace = workspace_for(&conversation, &project, &repo, &base);
        let state = AppState::new_test();
        let client = Arc::new(SubmittingPrDescriptionClient::failed(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation.id.clone(),
        ));
        let state = state.with_agent_client(client);

        let error = draft_agent_workspace_pr_description(
            &state,
            &conversation,
            &project,
            &workspace,
            &repo,
            &base,
        )
        .await
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("PR describer agent exited unsuccessfully"));
    }
}
