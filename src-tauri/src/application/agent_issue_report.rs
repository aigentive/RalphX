use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::application::AppState;
use crate::domain::entities::{
    AgentConversationWorkspace, ChatConversation, ChatConversationId, Project,
};
use crate::domain::services::GithubServiceTrait;
use crate::error::{AppError, AppResult};
use crate::infrastructure::agents::claude::config_path;
use crate::utils::runtime_log_paths;
use crate::utils::support_report_redactor::{
    redact_support_report_text, SupportReportRedactionContext, SupportReportRedactionSummary,
};

pub const PUBLIC_DEFAULT_SUPPORT_REPOSITORY: &str = "aigentive/ralphx.app";
const DEFAULT_LOG_MAX_BYTES: usize = 24 * 1024;
const MIN_LOG_MAX_BYTES: usize = 4 * 1024;
const MAX_LOG_MAX_BYTES: usize = 128 * 1024;
const MAX_LOG_SOURCES: usize = 4;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildAgentIssueReportInput {
    pub conversation_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_include_logs")]
    pub include_logs: bool,
    #[serde(default)]
    pub recent_errors_only: bool,
    #[serde(default = "default_log_max_bytes")]
    pub max_log_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitAgentIssueReportInput {
    pub conversation_id: String,
    pub repository: String,
    pub title: String,
    pub body_markdown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIssueReportDraft {
    pub conversation_id: String,
    pub project_id: String,
    pub generated_at: String,
    pub markdown: String,
    pub destination: AgentIssueReportDestination,
    pub redaction_summary: SupportReportRedactionSummary,
    pub sources: Vec<AgentIssueReportSource>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIssueReportDestination {
    pub repository: String,
    pub source: AgentIssueReportDestinationSource,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentIssueReportDestinationSource {
    Configured,
    PublicDefault,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIssueReportSource {
    pub label: String,
    pub included: bool,
    pub truncated: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentIssueReportSubmitResponse {
    pub repository: String,
    pub issue_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentIssueReportEnvironment {
    pub app_version: String,
    pub os_name: String,
    pub os_version: Option<String>,
    pub arch: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct ReportContext {
    conversation: ChatConversation,
    workspace: AgentConversationWorkspace,
    project: Project,
}

#[derive(Debug, Clone)]
struct ReportLogSource {
    label: String,
    body: String,
    truncated: bool,
}

pub(crate) struct ResolvedDestination {
    pub(crate) destination: AgentIssueReportDestination,
    pub(crate) warnings: Vec<String>,
}

fn default_include_logs() -> bool {
    true
}

fn default_log_max_bytes() -> usize {
    DEFAULT_LOG_MAX_BYTES
}

pub async fn build_agent_issue_report_draft(
    state: &AppState,
    input: BuildAgentIssueReportInput,
    environment: AgentIssueReportEnvironment,
) -> AppResult<AgentIssueReportDraft> {
    let conversation_id = parse_conversation_id(&input.conversation_id)?;
    let context = load_report_context(state, &conversation_id, input.project_id.as_deref()).await?;
    let redaction_context = SupportReportRedactionContext {
        project_root: Some(PathBuf::from(&context.project.working_directory)),
        workspace_root: Some(PathBuf::from(&context.workspace.worktree_path)),
        home_dir: dirs::home_dir(),
    };
    let mut redaction_summary = SupportReportRedactionSummary::default();
    let mut sources = Vec::new();
    let mut warnings = Vec::new();

    let logs = if input.include_logs {
        let options = LogCollectionOptions {
            conversation_id: context.conversation.id.as_str(),
            max_bytes: clamp_log_max_bytes(input.max_log_bytes),
            recent_errors_only: input.recent_errors_only,
        };
        match collect_report_logs(options) {
            Ok(collected) => {
                if collected.is_empty() {
                    warnings.push(
                        "No RalphX runtime log files were available under the fixed app log root."
                            .to_string(),
                    );
                }
                collected
            }
            Err(error) => {
                warnings.push(format!("Failed to read RalphX runtime logs: {error}"));
                Vec::new()
            }
        }
    } else {
        sources.push(AgentIssueReportSource {
            label: "logs".to_string(),
            included: false,
            truncated: false,
            detail: Some("Log inclusion disabled for this draft.".to_string()),
        });
        Vec::new()
    };

    let mut redacted_logs = Vec::new();
    for log in logs {
        let redacted = redact_support_report_text(&log.body, &redaction_context);
        redaction_summary.merge(redacted.summary);
        sources.push(AgentIssueReportSource {
            label: log.label.clone(),
            included: true,
            truncated: log.truncated,
            detail: if log.truncated {
                Some("Log content was truncated to the configured byte limit.".to_string())
            } else {
                None
            },
        });
        redacted_logs.push(ReportLogSource {
            label: log.label,
            body: redacted.text,
            truncated: log.truncated,
        });
    }

    let mut destination = resolve_agent_issue_report_destination();
    warnings.append(&mut destination.warnings);

    let markdown = render_report_markdown(
        &context,
        &environment,
        &destination.destination,
        &redacted_logs,
        &redaction_summary,
        &warnings,
    );

    Ok(AgentIssueReportDraft {
        conversation_id: context.conversation.id.as_str(),
        project_id: context.workspace.project_id.as_str().to_string(),
        generated_at: environment.generated_at.to_rfc3339(),
        markdown,
        destination: destination.destination,
        redaction_summary,
        sources,
        warnings,
    })
}

pub async fn submit_agent_issue_report(
    state: &AppState,
    input: SubmitAgentIssueReportInput,
) -> AppResult<AgentIssueReportSubmitResponse> {
    let _conversation_id = parse_conversation_id(&input.conversation_id)?;
    let repository = validate_github_repository(&input.repository)?.to_string();
    let title = validate_issue_title(&input.title)?;
    let body_markdown = validate_issue_body(&input.body_markdown)?;
    let github = state
        .github_service
        .as_ref()
        .ok_or_else(|| AppError::Infrastructure("GitHub issue submission is unavailable".into()))?;

    let issue_url = submit_agent_issue_report_with_service(
        github.as_ref(),
        &support_issue_report_working_dir(),
        &support_issue_report_body_dir(),
        &repository,
        &title,
        &body_markdown,
    )
    .await?;

    Ok(AgentIssueReportSubmitResponse {
        repository,
        issue_url,
    })
}

pub(crate) async fn submit_agent_issue_report_with_service(
    github: &dyn GithubServiceTrait,
    working_dir: &Path,
    body_dir: &Path,
    repository: &str,
    title: &str,
    body_markdown: &str,
) -> AppResult<String> {
    // These directories are RalphX-owned runtime artifact paths, not target project paths.
    // codeql[rust/path-injection]
    std::fs::create_dir_all(working_dir).map_err(|error| {
        AppError::Infrastructure(format!("Failed to prepare gh work dir: {error}"))
    })?;
    // Body files are generated under a fixed RalphX-owned artifact directory.
    // codeql[rust/path-injection]
    std::fs::create_dir_all(body_dir).map_err(|error| {
        AppError::Infrastructure(format!("Failed to prepare issue body directory: {error}"))
    })?;

    let body_file = body_dir.join(format!("issue-report-{}.md", uuid::Uuid::new_v4()));
    // The body file path is fixed-root plus a generated UUID filename.
    // codeql[rust/path-injection]
    tokio::fs::write(&body_file, body_markdown)
        .await
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to write issue body: {error}"))
        })?;

    let result = github
        .create_issue(working_dir, repository, title, &body_file)
        .await;
    // Best-effort cleanup of the generated issue body file.
    // codeql[rust/path-injection]
    let _ = std::fs::remove_file(&body_file);
    result
}

async fn load_report_context(
    state: &AppState,
    conversation_id: &ChatConversationId,
    expected_project_id: Option<&str>,
) -> AppResult<ReportContext> {
    let conversation = state
        .chat_conversation_repo
        .get_by_id(conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent conversation not found".to_string()))?;
    let workspace = state
        .agent_conversation_workspace_repo
        .get_by_conversation_id(conversation_id)
        .await?
        .ok_or_else(|| AppError::NotFound("Agent conversation workspace not found".to_string()))?;

    if let Some(expected_project_id) = expected_project_id {
        if workspace.project_id.as_str() != expected_project_id {
            return Err(AppError::Validation(
                "Selected project does not match the agent conversation workspace".to_string(),
            ));
        }
    }

    let project = state
        .project_repo
        .get_by_id(&workspace.project_id)
        .await?
        .ok_or_else(|| AppError::ProjectNotFound(workspace.project_id.as_str().to_string()))?;

    Ok(ReportContext {
        conversation,
        workspace,
        project,
    })
}

struct LogCollectionOptions {
    conversation_id: String,
    max_bytes: usize,
    recent_errors_only: bool,
}

fn collect_report_logs(options: LogCollectionOptions) -> std::io::Result<Vec<ReportLogSource>> {
    let log_root = runtime_log_paths::app_log_dir();
    let mut candidates = Vec::<PathBuf>::new();

    let stream_log = runtime_log_paths::stream_debug_log_file(&options.conversation_id);
    if stream_log.is_file() {
        candidates.push(stream_log);
    }

    // The app log root is a fixed RalphX-owned runtime path.
    // codeql[rust/path-injection]
    if let Ok(entries) = std::fs::read_dir(&log_root) {
        let mut app_logs = entries
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let file_type = entry.file_type().ok()?;
                if !file_type.is_file()
                    || path.extension().and_then(|ext| ext.to_str()) != Some("log")
                {
                    return None;
                }
                let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
                Some((modified, path))
            })
            .collect::<Vec<_>>();
        app_logs.sort_by(|a, b| b.0.cmp(&a.0));
        candidates.extend(
            app_logs
                .into_iter()
                .map(|(_, path)| path)
                .take(MAX_LOG_SOURCES.saturating_sub(candidates.len())),
        );
    }

    let mut logs = Vec::new();
    for path in candidates.into_iter().take(MAX_LOG_SOURCES) {
        let (mut body, truncated) = read_bounded_utf8(&path, options.max_bytes)?;
        if options.recent_errors_only {
            body = filter_error_lines(&body);
        }
        if body.trim().is_empty() {
            continue;
        }
        logs.push(ReportLogSource {
            label: log_label(&log_root, &path),
            body,
            truncated,
        });
    }

    Ok(logs)
}

fn read_bounded_utf8(path: &Path, max_bytes: usize) -> std::io::Result<(String, bool)> {
    // Candidate log paths are derived from fixed entries under the RalphX app log root.
    // codeql[rust/path-injection]
    let mut file = std::fs::File::open(path)?;
    let mut buffer = Vec::new();
    file.by_ref()
        .take(max_bytes.saturating_add(1) as u64)
        .read_to_end(&mut buffer)?;
    let truncated = buffer.len() > max_bytes;
    if truncated {
        buffer.truncate(max_bytes);
    }
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    while !text.is_char_boundary(text.len()) {
        text.pop();
    }
    Ok((text, truncated))
}

fn filter_error_lines(body: &str) -> String {
    body.lines()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("error") || lower.contains("warn") || lower.contains("panic")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn log_label(log_root: &Path, path: &Path) -> String {
    path.strip_prefix(log_root)
        .ok()
        .and_then(|relative| relative.to_str())
        .map(|relative| relative.to_string())
        .or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "unknown.log".to_string())
}

fn render_report_markdown(
    context: &ReportContext,
    environment: &AgentIssueReportEnvironment,
    destination: &AgentIssueReportDestination,
    logs: &[ReportLogSource],
    redaction_summary: &SupportReportRedactionSummary,
    warnings: &[String],
) -> String {
    let mut out = String::new();
    out.push_str("# RalphX Issue Report\n\n");
    out.push_str(
        "Review and edit this report before submitting. The submitted issue body is exactly the Markdown shown here.\n\n",
    );
    out.push_str("## User Notes\n\n");
    out.push_str("_Add a short description of what went wrong, what you expected, and steps to reproduce._\n\n");
    out.push_str("## Submission Target\n\n");
    out.push_str(&format!(
        "- Repository: `{}` ({})\n",
        destination.repository,
        match destination.source {
            AgentIssueReportDestinationSource::Configured => "configured destination",
            AgentIssueReportDestinationSource::PublicDefault => "public default destination",
        }
    ));
    out.push_str("\n## Environment\n\n");
    out.push_str(&format!(
        "- RalphX version: `{}`\n",
        environment.app_version
    ));
    out.push_str(&format!("- OS: `{}`\n", environment.os_name));
    out.push_str(&format!(
        "- OS version: `{}`\n",
        environment.os_version.as_deref().unwrap_or("unknown")
    ));
    out.push_str(&format!("- Architecture: `{}`\n", environment.arch));
    out.push_str(&format!(
        "- Generated at: `{}`\n",
        environment.generated_at.to_rfc3339()
    ));

    out.push_str("\n## Agent Conversation\n\n");
    out.push_str(&format!(
        "- Conversation ID: `{}`\n",
        context.conversation.id
    ));
    out.push_str(&format!(
        "- Context type: `{}`\n",
        context.conversation.context_type
    ));
    out.push_str(&format!(
        "- Context ID: `{}`\n",
        context.conversation.context_id
    ));
    if let Some(title) = context.conversation.title.as_deref() {
        out.push_str(&format!("- Conversation title: `{}`\n", title));
    }
    if let Some(harness) = context.conversation.provider_harness {
        out.push_str(&format!("- Provider harness: `{}`\n", harness));
    }
    if let Some(provider_session_id) = context.conversation.provider_session_id.as_deref() {
        out.push_str(&format!(
            "- Provider session ID: `{}`\n",
            provider_session_id
        ));
    }
    if let Some(agent_mode) = context.conversation.agent_mode {
        out.push_str(&format!("- Agent mode: `{}`\n", agent_mode));
    }

    out.push_str("\n## Workspace\n\n");
    out.push_str(&format!(
        "- Project ID: `{}`\n",
        context.workspace.project_id
    ));
    out.push_str("- Project name: _not included automatically_\n");
    out.push_str(&format!("- Workspace mode: `{}`\n", context.workspace.mode));
    out.push_str(&format!("- Branch: `{}`\n", context.workspace.branch_name));
    out.push_str(&format!("- Base ref: `{}`\n", context.workspace.base_ref));
    if let Some(base_commit) = context.workspace.base_commit.as_deref() {
        out.push_str(&format!("- Base commit: `{}`\n", base_commit));
    }
    out.push_str(&format!("- Worktree path: `{}`\n", "[AGENT_WORKSPACE]"));
    if let Some(publication_pr_number) = context.workspace.publication_pr_number {
        out.push_str(&format!(
            "- Publication PR number: `{}`\n",
            publication_pr_number
        ));
    }
    if let Some(publication_pr_status) = context.workspace.publication_pr_status.as_deref() {
        out.push_str(&format!(
            "- Publication PR status: `{}`\n",
            publication_pr_status
        ));
    }

    out.push_str("\n## Redaction\n\n");
    if redaction_summary.is_empty() {
        out.push_str("- No automated redactions were applied.\n");
    } else {
        for entry in &redaction_summary.replacements {
            out.push_str(&format!(
                "- `{}`: `{}` replacement(s)\n",
                entry.category, entry.count
            ));
        }
    }

    if !warnings.is_empty() {
        out.push_str("\n## Omissions And Warnings\n\n");
        for warning in warnings {
            out.push_str(&format!("- {warning}\n"));
        }
    }

    out.push_str("\n## Logs\n\n");
    if logs.is_empty() {
        out.push_str("_No logs included in this draft._\n");
    } else {
        for log in logs {
            out.push_str(&format!("### `{}`\n\n", log.label));
            if log.truncated {
                out.push_str("_This log was truncated before redaction._\n\n");
            }
            out.push_str("~~~text\n");
            out.push_str(&log.body);
            if !log.body.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("~~~\n\n");
        }
    }

    out
}

fn resolve_agent_issue_report_destination() -> ResolvedDestination {
    let path = config_path();
    // Main config path is a RalphX-owned runtime config path.
    // codeql[rust/path-injection]
    resolve_agent_issue_report_destination_from_config_result(std::fs::read_to_string(&path))
}

pub(crate) fn resolve_agent_issue_report_destination_from_config_result(
    config_result: std::io::Result<String>,
) -> ResolvedDestination {
    let mut warnings = Vec::new();
    match config_result {
        Ok(contents) => match configured_support_issue_repository_from_yaml(&contents) {
            Some(repository) => match validate_github_repository(&repository) {
                Ok(repository) => {
                    return ResolvedDestination {
                        destination: AgentIssueReportDestination {
                            repository: repository.to_string(),
                            source: AgentIssueReportDestinationSource::Configured,
                            is_default: false,
                        },
                        warnings,
                    };
                }
                Err(error) => warnings.push(format!(
                    "Configured support issue repository is invalid; using public default: {error}"
                )),
            },
            None => {}
        },
        Err(_) => warnings.push(
            "Support issue destination config was not found; using public default repository."
                .to_string(),
        ),
    }

    ResolvedDestination {
        destination: AgentIssueReportDestination {
            repository: PUBLIC_DEFAULT_SUPPORT_REPOSITORY.to_string(),
            source: AgentIssueReportDestinationSource::PublicDefault,
            is_default: true,
        },
        warnings,
    }
}

pub(crate) fn configured_support_issue_repository_from_yaml(contents: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct MinimalConfig {
        support_issue: Option<SupportIssueConfig>,
        issue_reporting: Option<SupportIssueConfig>,
    }

    #[derive(Deserialize)]
    struct SupportIssueConfig {
        github_repository: Option<String>,
        repository: Option<String>,
    }

    let config = serde_yaml::from_str::<MinimalConfig>(contents).ok()?;
    config
        .support_issue
        .or(config.issue_reporting)
        .and_then(|section| section.github_repository.or(section.repository))
}

fn parse_conversation_id(value: &str) -> AppResult<ChatConversationId> {
    value
        .parse::<ChatConversationId>()
        .map_err(|_| AppError::Validation("Invalid agent conversation ID".to_string()))
}

pub(crate) fn validate_github_repository(repository: &str) -> AppResult<&str> {
    let repository = repository.trim();
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(AppError::Validation(
            "GitHub repository must be in owner/name format".to_string(),
        ));
    };
    if owner.is_empty()
        || name.is_empty()
        || repository.split('/').count() != 2
        || !owner.chars().all(is_valid_github_repo_component_char)
        || !name.chars().all(is_valid_github_repo_component_char)
    {
        return Err(AppError::Validation(
            "GitHub repository must be a valid owner/name value".to_string(),
        ));
    }
    Ok(repository)
}

fn is_valid_github_repo_component_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')
}

fn validate_issue_title(title: &str) -> AppResult<String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::Validation(
            "Issue title cannot be empty".to_string(),
        ));
    }
    Ok(title.chars().take(180).collect())
}

fn validate_issue_body(body: &str) -> AppResult<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err(AppError::Validation(
            "Issue body cannot be empty".to_string(),
        ));
    }
    Ok(body.to_string())
}

fn clamp_log_max_bytes(value: usize) -> usize {
    value.clamp(MIN_LOG_MAX_BYTES, MAX_LOG_MAX_BYTES)
}

fn support_issue_report_body_dir() -> PathBuf {
    runtime_log_paths::app_artifact_dir().join("issue-reports")
}

fn support_issue_report_working_dir() -> PathBuf {
    runtime_log_paths::app_data_dir()
}
