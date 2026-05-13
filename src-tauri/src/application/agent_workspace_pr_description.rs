use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::application::git_service::git_cmd;
use crate::application::harness_runtime_registry::resolve_harness_agent_bootstrap;
use crate::application::{AppState, GitService};
use crate::domain::agents::{AgentConfig, AgentHarnessKind, AgentRole, DEFAULT_AGENT_HARNESS};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspacePrDescription, ChatConversation, ChatConversationId,
    Project,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::agent_names;
use crate::infrastructure::agents::claude::git_runtime_config;
use dashmap::DashMap;
use tracing::info;

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
const PR_DESCRIBER_SUBMIT_TOOL: &str = "submit_agent_workspace_pr_description";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PullRequestTemplateContext {
    source: &'static str,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentWorkspacePrDescriptionCacheKey {
    conversation_id: ChatConversationId,
    review_base: String,
    branch_head_sha: String,
    reviewable_commit_count: u32,
}

impl AgentWorkspacePrDescriptionCacheKey {
    pub(crate) fn new(
        conversation_id: ChatConversationId,
        review_base: impl Into<String>,
        branch_head_sha: impl Into<String>,
        reviewable_commit_count: u32,
    ) -> Option<Self> {
        if conversation_id.as_uuid().is_nil() {
            return None;
        }
        let review_base = review_base.into();
        let branch_head_sha = branch_head_sha.into();
        if review_base.trim().is_empty() || branch_head_sha.trim().is_empty() {
            return None;
        }
        Some(Self {
            conversation_id,
            review_base,
            branch_head_sha,
            reviewable_commit_count,
        })
    }

    fn cache_key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.conversation_id,
            self.review_base,
            self.branch_head_sha,
            self.reviewable_commit_count
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentWorkspacePrDescriptionCacheStatus {
    Hit,
    Coalesced,
    Miss,
    Disabled,
}

impl AgentWorkspacePrDescriptionCacheStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hit => "hit",
            Self::Coalesced => "coalesced",
            Self::Miss => "miss",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct AgentWorkspacePrDescriptionDraftOutcome {
    pub(crate) description: AgentWorkspacePrDescription,
    pub(crate) cache_status: AgentWorkspacePrDescriptionCacheStatus,
    pub(crate) cache_age_ms: Option<u128>,
    pub(crate) cache_wait_ms: u128,
}

#[derive(Debug, Clone)]
struct AgentWorkspacePrDescriptionCacheEntry {
    inserted_at: Instant,
    description: AgentWorkspacePrDescription,
}

fn agent_workspace_pr_description_cache_ttl() -> Duration {
    Duration::from_millis(git_runtime_config().workspace_pr_description_cache_ttl_ms)
}

fn agent_workspace_pr_description_cache(
) -> &'static DashMap<String, AgentWorkspacePrDescriptionCacheEntry> {
    static CACHE: OnceLock<DashMap<String, AgentWorkspacePrDescriptionCacheEntry>> =
        OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

fn agent_workspace_pr_description_locks() -> &'static DashMap<String, Arc<tokio::sync::Mutex<()>>> {
    static LOCKS: OnceLock<DashMap<String, Arc<tokio::sync::Mutex<()>>>> = OnceLock::new();
    LOCKS.get_or_init(DashMap::new)
}

fn cached_agent_workspace_pr_description(
    key: &AgentWorkspacePrDescriptionCacheKey,
) -> Option<(AgentWorkspacePrDescription, u128)> {
    let ttl = agent_workspace_pr_description_cache_ttl();
    if ttl.is_zero() {
        return None;
    }
    let cache_key = key.cache_key();
    let Some(entry) = agent_workspace_pr_description_cache().get(&cache_key) else {
        return None;
    };
    let age = entry.inserted_at.elapsed();
    if age <= ttl {
        return Some((entry.description.clone(), age.as_millis()));
    }
    drop(entry);
    agent_workspace_pr_description_cache().remove(&cache_key);
    None
}

fn store_agent_workspace_pr_description(
    key: &AgentWorkspacePrDescriptionCacheKey,
    description: &AgentWorkspacePrDescription,
) {
    if agent_workspace_pr_description_cache_ttl().is_zero() {
        return;
    }
    agent_workspace_pr_description_cache().insert(
        key.cache_key(),
        AgentWorkspacePrDescriptionCacheEntry {
            inserted_at: Instant::now(),
            description: description.clone(),
        },
    );
}

pub(crate) fn invalidate_agent_workspace_pr_description_cache(
    conversation_id: &ChatConversationId,
) {
    if conversation_id.as_uuid().is_nil() {
        return;
    }
    let prefix = format!("{conversation_id}:");
    let keys = agent_workspace_pr_description_cache()
        .iter()
        .filter_map(|entry| {
            entry
                .key()
                .starts_with(&prefix)
                .then(|| entry.key().clone())
        })
        .collect::<Vec<_>>();
    for key in keys {
        agent_workspace_pr_description_cache().remove(&key);
    }
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
    let total_started = Instant::now();
    state
        .agent_conversation_workspace_repo
        .clear_pr_description(&workspace.conversation_id)
        .await?;

    let context_started = Instant::now();
    let review_range = format!("{review_base}..HEAD");
    let template_fut = async {
        Ok::<_, AppError>(read_pull_request_template_context(project, workspace_path).await)
    };
    let diff_stats_fut = GitService::get_diff_stats_between(workspace_path, review_base, "HEAD");
    let commits_fut = GitService::get_commits_between(workspace_path, review_base, "HEAD");
    let name_status_fut = run_git_text_owned(
        workspace_path,
        vec![
            "diff".to_string(),
            "--find-renames".to_string(),
            "--name-status".to_string(),
            review_range.clone(),
        ],
    );
    let diff_stat_fut = run_git_text_owned(
        workspace_path,
        vec![
            "diff".to_string(),
            "--find-renames".to_string(),
            "--stat".to_string(),
            review_range.clone(),
        ],
    );
    let patch_excerpt_fut = run_git_text_owned(
        workspace_path,
        vec![
            "diff".to_string(),
            "--find-renames".to_string(),
            "--minimal".to_string(),
            "--no-ext-diff".to_string(),
            review_range,
        ],
    );
    let conversation_context_fut = build_conversation_context(state, conversation);
    let (
        template,
        diff_stats,
        commits,
        name_status,
        diff_stat,
        patch_excerpt,
        conversation_context,
    ) = tokio::try_join!(
        template_fut,
        diff_stats_fut,
        commits_fut,
        name_status_fut,
        diff_stat_fut,
        patch_excerpt_fut,
        conversation_context_fut
    )?;
    info!(
        target: "ralphx_lib::application::agent_workspace_pr_description",
        conversation_id = %workspace.conversation_id,
        project_id = %project.id,
        branch = %workspace.branch_name,
        review_base,
        elapsed_ms = context_started.elapsed().as_millis(),
        commits = commits.len(),
        files_changed = diff_stats.files_changed,
        patch_excerpt_chars = patch_excerpt.chars().count(),
        "Collected agent workspace PR description context"
    );
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
    ensure_pr_describer_submit_tool_available(helper_harness, &bootstrap.plugin_dir)?;

    let spawn_started = Instant::now();
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
    info!(
        target: "ralphx_lib::application::agent_workspace_pr_description",
        conversation_id = %workspace.conversation_id,
        project_id = %project.id,
        branch = %workspace.branch_name,
        harness = %helper_harness,
        elapsed_ms = spawn_started.elapsed().as_millis(),
        "Spawned agent workspace PR describer helper"
    );

    let wait_started = Instant::now();
    let output = agent_client
        .wait_for_completion(&output)
        .await
        .map_err(|error| AppError::Infrastructure(format!("PR describer agent failed: {error}")))?;
    info!(
        target: "ralphx_lib::application::agent_workspace_pr_description",
        conversation_id = %workspace.conversation_id,
        project_id = %project.id,
        branch = %workspace.branch_name,
        harness = %helper_harness,
        elapsed_ms = wait_started.elapsed().as_millis(),
        success = output.success,
        "Agent workspace PR describer helper completed"
    );
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
        return Err(pr_describer_missing_submission_error(&output));
    };

    validate_agent_workspace_pr_description_body(&description.body_markdown)?;
    info!(
        target: "ralphx_lib::application::agent_workspace_pr_description",
        conversation_id = %workspace.conversation_id,
        project_id = %project.id,
        branch = %workspace.branch_name,
        review_base,
        elapsed_ms = total_started.elapsed().as_millis(),
        body_chars = description.body_markdown.chars().count(),
        has_title = description.title.is_some(),
        "Drafted agent workspace PR description"
    );
    Ok(AgentWorkspacePrDescription::new(
        description.title,
        description.body_markdown,
    ))
}

pub(crate) async fn get_or_draft_agent_workspace_pr_description(
    state: &AppState,
    conversation: &ChatConversation,
    project: &Project,
    workspace: &AgentConversationWorkspace,
    workspace_path: &Path,
    review_base: &str,
    key: AgentWorkspacePrDescriptionCacheKey,
) -> AppResult<AgentWorkspacePrDescriptionDraftOutcome> {
    if agent_workspace_pr_description_cache_ttl().is_zero() {
        let description = draft_agent_workspace_pr_description(
            state,
            conversation,
            project,
            workspace,
            workspace_path,
            review_base,
        )
        .await?;
        return Ok(AgentWorkspacePrDescriptionDraftOutcome {
            description,
            cache_status: AgentWorkspacePrDescriptionCacheStatus::Disabled,
            cache_age_ms: None,
            cache_wait_ms: 0,
        });
    }

    if let Some((description, age_ms)) = cached_agent_workspace_pr_description(&key) {
        return Ok(AgentWorkspacePrDescriptionDraftOutcome {
            description,
            cache_status: AgentWorkspacePrDescriptionCacheStatus::Hit,
            cache_age_ms: Some(age_ms),
            cache_wait_ms: 0,
        });
    }

    let lock = agent_workspace_pr_description_locks()
        .entry(key.cache_key())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let wait_started = Instant::now();
    let _guard = lock.lock().await;
    let wait_ms = wait_started.elapsed().as_millis();

    if let Some((description, age_ms)) = cached_agent_workspace_pr_description(&key) {
        return Ok(AgentWorkspacePrDescriptionDraftOutcome {
            description,
            cache_status: AgentWorkspacePrDescriptionCacheStatus::Coalesced,
            cache_age_ms: Some(age_ms),
            cache_wait_ms: wait_ms,
        });
    }

    let description = draft_agent_workspace_pr_description(
        state,
        conversation,
        project,
        workspace,
        workspace_path,
        review_base,
    )
    .await?;
    store_agent_workspace_pr_description(&key, &description);

    Ok(AgentWorkspacePrDescriptionDraftOutcome {
        description,
        cache_status: AgentWorkspacePrDescriptionCacheStatus::Miss,
        cache_age_ms: None,
        cache_wait_ms: wait_ms,
    })
}

fn ensure_pr_describer_submit_tool_available(
    harness: AgentHarnessKind,
    plugin_dir: &Path,
) -> AppResult<()> {
    if harness != AgentHarnessKind::Codex {
        return Ok(());
    }

    ensure_codex_pr_describer_prompt_contract(plugin_dir)?;
    let overrides = crate::infrastructure::agents::codex::build_codex_mcp_overrides(
        plugin_dir,
        agent_names::AGENT_PR_DESCRIBER,
        false,
        None,
    )
    .map_err(|error| {
        AppError::Infrastructure(format!(
            "PR describer Codex MCP preflight failed for {}: {error}",
            plugin_dir.display()
        ))
    })?;

    if codex_pr_describer_overrides_expose_submit_tool(&overrides) {
        return Ok(());
    }

    Err(AppError::Infrastructure(format!(
        "PR describer Codex MCP preflight failed: required tool `{PR_DESCRIBER_SUBMIT_TOOL}` is not exposed for plugin dir {}",
        plugin_dir.display()
    )))
}

fn ensure_codex_pr_describer_prompt_contract(plugin_dir: &Path) -> AppResult<()> {
    let project_root =
        crate::infrastructure::agents::harness_agent_catalog::resolve_project_root_from_plugin_dir(
            plugin_dir,
        );
    let prompt = crate::infrastructure::agents::harness_agent_catalog::load_harness_agent_prompt(
        &project_root,
        agent_names::SHORT_PR_DESCRIBER,
        crate::infrastructure::agents::harness_agent_catalog::AgentPromptHarness::Codex,
    )
    .ok_or_else(|| {
        AppError::Infrastructure(format!(
            "PR describer Codex prompt contract is missing for plugin dir {}",
            plugin_dir.display()
        ))
    })?;

    if prompt.contains(PR_DESCRIBER_SUBMIT_TOOL) {
        return Ok(());
    }

    Err(AppError::Infrastructure(format!(
        "PR describer Codex prompt contract does not mention required tool `{PR_DESCRIBER_SUBMIT_TOOL}` for plugin dir {}",
        plugin_dir.display()
    )))
}

fn codex_pr_describer_overrides_expose_submit_tool(overrides: &[String]) -> bool {
    let enabled_tools_ok = overrides.iter().any(|entry| {
        override_json_value(entry, ".enabled_tools")
            .is_some_and(|value| json_string_array_contains(value, PR_DESCRIBER_SUBMIT_TOOL))
    });
    let args_override = overrides
        .iter()
        .find_map(|entry| override_json_value(entry, ".args"));
    let stdio_args_ok = args_override
        .is_none_or(|value| codex_stdio_args_allow_required_tool(value, PR_DESCRIBER_SUBMIT_TOOL));

    enabled_tools_ok && stdio_args_ok
}

fn override_json_value<'a>(entry: &'a str, key_suffix: &str) -> Option<&'a str> {
    let (key, value) = entry.split_once('=')?;
    key.ends_with(key_suffix).then_some(value)
}

fn json_string_array_contains(value: &str, needle: &str) -> bool {
    serde_json::from_str::<Vec<String>>(value)
        .map(|values| values.iter().any(|value| value == needle))
        .unwrap_or(false)
}

fn codex_stdio_args_allow_required_tool(args_json: &str, required_tool: &str) -> bool {
    serde_json::from_str::<Vec<String>>(args_json)
        .map(|args| {
            args.iter().any(|arg| {
                arg.strip_prefix("--allowed-tools=")
                    .is_some_and(|tools| tools.split(',').any(|tool| tool == required_tool))
            })
        })
        .unwrap_or(false)
}

fn pr_describer_missing_submission_error(output: &crate::domain::agents::AgentOutput) -> AppError {
    let raw_output = output.content.trim();
    let base = if pr_describer_output_reports_missing_submit_tool(raw_output) {
        format!(
            "PR describer infrastructure error: required tool `{PR_DESCRIBER_SUBMIT_TOOL}` was unavailable to the agent"
        )
    } else {
        "PR describer agent completed without submitting a PR description".to_string()
    };

    if raw_output.is_empty() {
        return AppError::Infrastructure(base);
    }

    AppError::Infrastructure(format!("{base}. Raw output: {raw_output}"))
}

fn pr_describer_output_reports_missing_submit_tool(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains(PR_DESCRIBER_SUBMIT_TOOL)
        && (lower.contains("not available")
            || lower.contains("unavailable")
            || lower.contains("cannot submit")
            || lower.contains("can't submit"))
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

async fn run_git_text_owned(repo: &Path, args: Vec<String>) -> AppResult<String> {
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_git_text(repo, &arg_refs).await
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
    use std::fs;
    use std::pin::Pin;
    use std::process::Command;

    use async_trait::async_trait;
    use chrono::{Duration, Utc};
    use futures::{stream, Stream};
    use tempfile::TempDir;

    use crate::domain::agents::{
        AgentConfig, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse, AgentResult,
        AgentRole, AgenticClient, ClientCapabilities, LogicalEffort, ResponseChunk,
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
        submit_on_success: bool,
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
                submit_on_success: true,
                capabilities: ClientCapabilities::mock(),
                spawned_configs: tokio::sync::Mutex::new(Vec::new()),
            }
        }

        fn success_without_submission(
            repo: Arc<dyn AgentConversationWorkspaceRepository>,
            conversation_id: crate::domain::entities::ChatConversationId,
            output: impl Into<String>,
        ) -> Self {
            Self {
                repo,
                conversation_id,
                title: None,
                body_markdown: String::new(),
                output: AgentOutput::success(output),
                submit_on_success: false,
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
                submit_on_success: false,
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
            if self.output.success && self.submit_on_success {
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

    fn test_cache_key() -> AgentWorkspacePrDescriptionCacheKey {
        AgentWorkspacePrDescriptionCacheKey::new(
            ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
            "base-sha",
            "head-sha",
            2,
        )
        .expect("test key should be cacheable")
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
    fn pr_description_cache_rejects_uncacheable_keys() {
        assert!(AgentWorkspacePrDescriptionCacheKey::new(
            ChatConversationId::from_string(uuid::Uuid::nil().to_string()),
            "base",
            "head",
            1,
        )
        .is_none());
        assert!(AgentWorkspacePrDescriptionCacheKey::new(
            ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
            "",
            "head",
            1,
        )
        .is_none());
        assert!(AgentWorkspacePrDescriptionCacheKey::new(
            ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
            "base",
            " ",
            1,
        )
        .is_none());
    }

    #[test]
    fn pr_description_cache_hits_and_invalidates_by_conversation() {
        let key = test_cache_key();
        invalidate_agent_workspace_pr_description_cache(&key.conversation_id);
        let description = AgentWorkspacePrDescription::new(
            Some("Draft title".to_string()),
            "## Summary\n\nCached body".to_string(),
        );

        store_agent_workspace_pr_description(&key, &description);

        let (cached, age_ms) =
            cached_agent_workspace_pr_description(&key).expect("cached description should hit");
        assert_eq!(cached, description);
        assert!(age_ms < 1_000);

        invalidate_agent_workspace_pr_description_cache(&key.conversation_id);
        assert!(cached_agent_workspace_pr_description(&key).is_none());
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

    #[test]
    fn codex_pr_describer_submit_tool_preflight_requires_enabled_tool_and_allowed_arg() {
        let missing_enabled_tools = vec![format!(
            "mcp_servers.ralphx.args=[\"server\",\"--allowed-tools={PR_DESCRIBER_SUBMIT_TOOL}\"]"
        )];
        assert!(!codex_pr_describer_overrides_expose_submit_tool(
            &missing_enabled_tools
        ));

        let missing_allowed_arg = vec![
            format!("mcp_servers.ralphx.enabled_tools=[\"{PR_DESCRIBER_SUBMIT_TOOL}\"]"),
            "mcp_servers.ralphx.args=[\"server\",\"--allowed-tools=other_tool\"]".to_string(),
        ];
        assert!(!codex_pr_describer_overrides_expose_submit_tool(
            &missing_allowed_arg
        ));

        let complete_surface = vec![
            format!("mcp_servers.ralphx.enabled_tools=[\"{PR_DESCRIBER_SUBMIT_TOOL}\"]"),
            format!(
                "mcp_servers.ralphx.args=[\"server\",\"--allowed-tools={PR_DESCRIBER_SUBMIT_TOOL}\"]"
            ),
        ];
        assert!(codex_pr_describer_overrides_expose_submit_tool(
            &complete_surface
        ));
    }

    #[test]
    fn pr_describer_submit_tool_preflight_skips_non_codex_harnesses() {
        let dir = tempfile::TempDir::new().expect("create temp dir");

        ensure_pr_describer_submit_tool_available(AgentHarnessKind::Claude, dir.path())
            .expect("Claude PR describer should not require Codex submit-tool preflight");
    }

    #[test]
    fn codex_pr_describer_prompt_contract_rejects_missing_or_invalid_prompt() {
        let dir = tempfile::TempDir::new().expect("create temp dir");
        let root = dir.path();
        let plugin_dir = root.join("plugins/app");
        let agent_root = root.join("agents").join(agent_names::SHORT_PR_DESCRIBER);
        let shared_prompt_dir = agent_root.join("shared");
        fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        fs::create_dir_all(&shared_prompt_dir).expect("create shared prompt dir");
        fs::write(
            agent_root.join("agent.yaml"),
            format!("name: {}\nrole: utility\n", agent_names::SHORT_PR_DESCRIBER),
        )
        .expect("write agent definition");

        let error = ensure_codex_pr_describer_prompt_contract(&plugin_dir)
            .expect_err("missing prompt should fail preflight")
            .to_string();
        assert!(error.contains("prompt contract is missing"));

        fs::write(
            shared_prompt_dir.join("prompt.md"),
            "Draft a reviewer-focused PR description.",
        )
        .expect("write prompt without submit tool");
        let error = ensure_codex_pr_describer_prompt_contract(&plugin_dir)
            .expect_err("prompt without submit tool should fail preflight")
            .to_string();
        assert!(error.contains("does not mention required tool"));

        fs::write(
            shared_prompt_dir.join("prompt.md"),
            format!("Call `{PR_DESCRIBER_SUBMIT_TOOL}` with the final PR body."),
        )
        .expect("write prompt with submit tool");
        ensure_codex_pr_describer_prompt_contract(&plugin_dir)
            .expect("prompt with submit tool should satisfy preflight");
        ensure_pr_describer_submit_tool_available(AgentHarnessKind::Codex, &plugin_dir)
            .expect("valid Codex PR describer surface should satisfy full preflight");
    }

    #[test]
    fn pr_describer_missing_submission_error_preserves_raw_output_without_tool_hint() {
        let output = AgentOutput::success("Generated a body but did not call the submit tool.");

        let error = pr_describer_missing_submission_error(&output).to_string();

        assert!(error.contains("completed without submitting a PR description"));
        assert!(error.contains("Raw output: Generated a body"));
    }

    #[test]
    fn pr_describer_missing_submission_error_omits_raw_section_for_empty_output() {
        let output = AgentOutput::success("   \n");

        let error = pr_describer_missing_submission_error(&output).to_string();

        assert!(error.contains("completed without submitting a PR description"));
        assert!(!error.contains("Raw output:"));
    }

    #[test]
    fn pr_describer_tool_unavailable_detector_accepts_observed_wording() {
        for output in [
            format!(
                "I can't submit this because `{PR_DESCRIBER_SUBMIT_TOOL}` is not available in this session's tools."
            ),
            format!(
                "I cannot submit this because `{PR_DESCRIBER_SUBMIT_TOOL}` was unavailable."
            ),
            format!("I cannot submit because `{PR_DESCRIBER_SUBMIT_TOOL}` was missing."),
            format!("I can't submit because `{PR_DESCRIBER_SUBMIT_TOOL}` was missing."),
        ] {
            assert!(pr_describer_output_reports_missing_submit_tool(&output));
        }

        assert!(!pr_describer_output_reports_missing_submit_tool(
            "I drafted the PR description but forgot to submit it."
        ));
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
        assert_eq!(config.model.as_deref(), Some("haiku"));
        assert_eq!(config.logical_effort, Some(LogicalEffort::Medium));
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
        assert_eq!(configs[0].model.as_deref(), Some("gpt-5.4-mini"));
        assert_eq!(configs[0].logical_effort, Some(LogicalEffort::Medium));
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

    #[tokio::test]
    async fn draft_pr_description_surfaces_raw_tool_unavailable_output_when_agent_submits_nothing()
    {
        let (_temp_dir, repo, base) = create_reviewable_repo();
        let project = project_for(&repo);
        let mut conversation = conversation_for(&project);
        conversation.provider_harness = Some(AgentHarnessKind::Codex);
        let workspace = workspace_for(&conversation, &project, &repo, &base);
        let state = AppState::new_test();
        let raw_output = format!(
            "I cannot submit this because `{PR_DESCRIBER_SUBMIT_TOOL}` is not available in this session's tools."
        );
        let codex_client = Arc::new(SubmittingPrDescriptionClient::success_without_submission(
            Arc::clone(&state.agent_conversation_workspace_repo),
            conversation.id.clone(),
            raw_output.clone(),
        ));
        let state = state.with_harness_agent_client(AgentHarnessKind::Codex, codex_client);

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
        let error = error.to_string();

        assert!(error.contains("PR describer infrastructure error"));
        assert!(
            error.contains(&raw_output),
            "raw model output should be surfaced for publish failure diagnostics: {error}"
        );
    }
}
