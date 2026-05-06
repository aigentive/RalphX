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

    let runtime = state.resolve_pr_describer_runtime().await;
    let agent_client = Arc::clone(&runtime.client);
    let helper_harness = runtime.harness.unwrap_or(DEFAULT_AGENT_HARNESS);
    let bootstrap = resolve_harness_agent_bootstrap(
        helper_harness,
        agent_names::AGENT_PR_DESCRIBER,
        workspace_path.to_path_buf(),
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
            return Ok(truncate_with_notice(
                &context,
                MAX_CONVERSATION_CONTEXT_CHARS,
                "\n\n[Conversation context truncated by RalphX]\n",
            ));
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
         If the diff excerpt is truncated or ambiguous, describe that uncertainty in the Risks / Follow-Ups section.\n\
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
        name_status = escape_xml_text(&truncate_with_notice(
            ctx.name_status,
            MAX_NAME_STATUS_CHARS,
            "\n[Name-status output truncated by RalphX]\n",
        )),
        diff_stat = escape_xml_text(&truncate_with_notice(
            ctx.diff_stat,
            MAX_STAT_CHARS,
            "\n[Diff stat output truncated by RalphX]\n",
        )),
        patch_excerpt = escape_xml_text(&truncate_with_notice(
            ctx.patch_excerpt,
            MAX_PATCH_EXCERPT_CHARS,
            "\n[Patch excerpt truncated by RalphX]\n",
        )),
        conversation_context = escape_xml_text(ctx.conversation_context),
    )
}

fn format_commit_summaries(commits: &[crate::application::git_service::CommitInfo]) -> String {
    if commits.is_empty() {
        return "No commit summaries were available.".to_string();
    }

    let mut lines = commits
        .iter()
        .take(MAX_COMMIT_SUMMARIES)
        .map(|commit| {
            format!(
                "- {} {} ({}, {})",
                commit.short_sha, commit.message, commit.author, commit.timestamp
            )
        })
        .collect::<Vec<_>>();
    if commits.len() > MAX_COMMIT_SUMMARIES {
        lines.push(format!(
            "- [{} additional commits omitted by RalphX]",
            commits.len() - MAX_COMMIT_SUMMARIES
        ));
    }
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

fn truncate_with_notice(text: &str, max_chars: usize, notice: &str) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    format!("{}{}", truncate_chars(text, max_chars), notice)
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

    #[test]
    fn validation_rejects_empty_body() {
        let error = validate_agent_workspace_pr_description_body("  ").unwrap_err();
        assert!(error.to_string().contains("cannot be empty"));
    }

    #[test]
    fn fallback_template_has_expected_sections() {
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## Summary"));
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## User Impact"));
        assert!(DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.contains("## Risks / Follow-Ups"));
    }

    #[test]
    fn truncation_adds_notice_when_needed() {
        let truncated = truncate_with_notice("abcdef", 3, "[truncated]");
        assert_eq!(truncated, "abc[truncated]");
    }
}
