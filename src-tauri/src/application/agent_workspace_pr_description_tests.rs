use super::*;
use std::fs;
use std::pin::Pin;
use std::process::Command;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use futures::{stream, Stream};
use tempfile::TempDir;

use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentHarnessKind, AgentOutput, AgentResponse, AgentResult, AgentRole,
    AgenticClient, ClientCapabilities, LogicalEffort, ResponseChunk,
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
                .save_pr_metadata_decision(
                    &self.conversation_id,
                    AgentWorkspacePrMetadataDecision::patch(
                        self.title.clone(),
                        Some(self.body_markdown.clone()),
                    )
                    .expect("test PR metadata decision should be valid"),
                )
                .await
                .expect("test PR metadata decision should save");
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
    AgentWorkspacePrDescriptionCacheKey::for_target(
        ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
        "base-sha",
        "head-sha",
        2,
        &ResolvedAgentWorkspacePrTarget::NewPr,
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
    assert!(AgentWorkspacePrDescriptionCacheKey::for_target(
        ChatConversationId::from_string(uuid::Uuid::nil().to_string()),
        "base",
        "head",
        1,
        &ResolvedAgentWorkspacePrTarget::NewPr,
    )
    .is_none());
    assert!(AgentWorkspacePrDescriptionCacheKey::for_target(
        ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
        "",
        "head",
        1,
        &ResolvedAgentWorkspacePrTarget::NewPr,
    )
    .is_none());
    assert!(AgentWorkspacePrDescriptionCacheKey::for_target(
        ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
        "base",
        " ",
        1,
        &ResolvedAgentWorkspacePrTarget::NewPr,
    )
    .is_none());
}

#[test]
fn pr_description_cache_status_labels_are_stable() {
    assert_eq!(AgentWorkspacePrDescriptionCacheStatus::Hit.as_str(), "hit");
    assert_eq!(
        AgentWorkspacePrDescriptionCacheStatus::Coalesced.as_str(),
        "coalesced"
    );
    assert_eq!(
        AgentWorkspacePrDescriptionCacheStatus::Miss.as_str(),
        "miss"
    );
    assert_eq!(
        AgentWorkspacePrDescriptionCacheStatus::Disabled.as_str(),
        "disabled"
    );
}

#[test]
fn pr_description_cache_hits_and_invalidates_by_conversation() {
    let key = test_cache_key();
    invalidate_agent_workspace_pr_description_cache(&key.conversation_id);
    let decision = AgentWorkspacePrMetadataDecision::patch(
        Some("Draft title".to_string()),
        Some("## Summary\n\nCached body".to_string()),
    )
    .unwrap();

    store_agent_workspace_pr_metadata_decision(&key, &decision);

    let (cached, age_ms) =
        cached_agent_workspace_pr_metadata_decision(&key).expect("cached decision should hit");
    assert_eq!(cached, decision);
    assert!(age_ms < 1_000);

    invalidate_agent_workspace_pr_description_cache(&key.conversation_id);
    assert!(cached_agent_workspace_pr_metadata_decision(&key).is_none());
}

#[test]
fn pr_description_cache_invalidation_is_conversation_scoped() {
    let first_key = test_cache_key();
    let second_key = test_cache_key();
    invalidate_agent_workspace_pr_description_cache(&first_key.conversation_id);
    invalidate_agent_workspace_pr_description_cache(&second_key.conversation_id);

    let first_decision = AgentWorkspacePrMetadataDecision::Preserve;
    let second_decision =
        AgentWorkspacePrMetadataDecision::patch(Some("Second title".to_string()), None).unwrap();

    store_agent_workspace_pr_metadata_decision(&first_key, &first_decision);
    store_agent_workspace_pr_metadata_decision(&second_key, &second_decision);
    invalidate_agent_workspace_pr_description_cache(&first_key.conversation_id);

    assert!(cached_agent_workspace_pr_metadata_decision(&first_key).is_none());
    let (cached_second, _) = cached_agent_workspace_pr_metadata_decision(&second_key)
        .expect("other conversation cache entry should remain");
    assert_eq!(cached_second, second_decision);

    invalidate_agent_workspace_pr_description_cache(&second_key.conversation_id);
}

#[tokio::test]
async fn get_or_draft_pr_description_caches_miss_then_hit() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    let target = ResolvedAgentWorkspacePrTarget::NewPr;
    let key = AgentWorkspacePrDescriptionCacheKey::for_target(
        conversation.id.clone(),
        base.clone(),
        head,
        1,
        &target,
    )
    .expect("cache key should be valid");
    invalidate_agent_workspace_pr_description_cache(&conversation.id);

    let state = AppState::new_test();
    let client = Arc::new(SubmittingPrDescriptionClient::success(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation.id.clone(),
        Some("Cached draft title".to_string()),
        "## Summary\n\nCached draft body.",
    ));
    let state = state.with_agent_client(client.clone());

    let first = get_or_draft_agent_workspace_pr_metadata_decision(
        &state,
        &conversation,
        &project,
        &workspace,
        &repo,
        &base,
        &target,
        key.clone(),
    )
    .await
    .expect("first draft should succeed");

    assert_eq!(
        first.cache_status,
        AgentWorkspacePrDescriptionCacheStatus::Miss
    );
    assert!(first.cache_age_ms.is_none());
    let AgentWorkspacePrMetadataDecision::Patch { title, .. } = &first.decision else {
        panic!("new PR draft should be a metadata patch");
    };
    assert_eq!(title.as_deref(), Some("Cached draft title"));
    assert_eq!(client.spawned_configs().await.len(), 1);

    let second = get_or_draft_agent_workspace_pr_metadata_decision(
        &state,
        &conversation,
        &project,
        &workspace,
        &repo,
        &base,
        &target,
        key,
    )
    .await
    .expect("second draft should hit cache");

    assert_eq!(
        second.cache_status,
        AgentWorkspacePrDescriptionCacheStatus::Hit
    );
    assert!(second.cache_age_ms.is_some());
    assert_eq!(second.cache_wait_ms, 0);
    let AgentWorkspacePrMetadataDecision::Patch { body_markdown, .. } = &second.decision else {
        panic!("new PR draft should be a metadata patch");
    };
    assert_eq!(
        body_markdown.as_deref(),
        Some("## Summary\n\nCached draft body.")
    );
    assert_eq!(
        client.spawned_configs().await.len(),
        1,
        "cache hit should not spawn another PR describer"
    );

    invalidate_agent_workspace_pr_description_cache(&conversation.id);
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
        target: &ResolvedAgentWorkspacePrTarget::NewPr,
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
    assert!(
        prompt.contains("Describe the final net changes shown by the diff context"),
        "prompt should steer the PR describer away from commit chronology"
    );
    assert!(
            prompt.contains("<commit_summaries secondary=\"true\" order=\"oldest_first\" merge_commits=\"omitted\">"),
            "commit context should be marked as secondary evidence"
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
fn commit_summaries_are_oldest_first_and_omit_merge_noise() {
    let commits = vec![
        crate::application::git_service::CommitInfo {
            sha: "3".repeat(40),
            short_sha: "3333333".to_string(),
            message: "Polish latest review feedback".to_string(),
            author: "Agent".to_string(),
            timestamp: "2026-05-06T00:03:00Z".to_string(),
        },
        crate::application::git_service::CommitInfo {
            sha: "2".repeat(40),
            short_sha: "2222222".to_string(),
            message: "Merge branch 'main' into feature".to_string(),
            author: "Agent".to_string(),
            timestamp: "2026-05-06T00:02:00Z".to_string(),
        },
        crate::application::git_service::CommitInfo {
            sha: "1".repeat(40),
            short_sha: "1111111".to_string(),
            message: "Add publish description net-diff context".to_string(),
            author: "Agent".to_string(),
            timestamp: "2026-05-06T00:01:00Z".to_string(),
        },
    ];

    let summary = format_commit_summaries(&commits);

    assert!(summary.contains("Add publish description net-diff context"));
    assert!(summary.contains("Polish latest review feedback"));
    assert!(!summary.contains("Merge branch"));
    let first_summary = summary
        .find("Add publish description net-diff context")
        .expect("oldest commit summary should be present");
    let latest_summary = summary
        .find("Polish latest review feedback")
        .expect("latest commit summary should be present");
    assert!(
        first_summary < latest_summary,
        "commit summaries should be oldest-first so the latest iteration is not first"
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
fn pr_describer_missing_submission_error_omits_raw_output_without_tool_hint() {
    let output = AgentOutput::success("Generated a body but did not call the submit tool.");

    let error = pr_describer_missing_submission_error(&output).to_string();

    assert!(error.contains("completed without submitting a PR description"));
    assert!(!error.contains("Raw output: Generated a body"));
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
    assert!(config.prompt.contains("decision=preserve"));
    assert!(config.prompt.contains("decision=patch"));
    assert!(config.prompt.contains("untrusted evidence"));
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
async fn draft_pr_description_recovers_literal_tool_call_output() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let state = AppState::new_test();
    let raw_output = format!(
            "## Summary\n\nRecovered body\n\n<call_tool>\n\
             <tool_name>{PR_DESCRIBER_SUBMIT_TOOL}</tool_name>\n\
             <tool_parameters>\n\
             <parameter name=\"conversation_id\">{}</parameter>\n\
             <parameter name=\"title\">Recovered title</parameter>\n\
             <parameter name=\"body_markdown\">## Summary\n\nRecovered body &amp; context</parameter>\n\
             </tool_parameters>\n\
             </call_tool>",
            conversation.id
        );
    let client = Arc::new(SubmittingPrDescriptionClient::success_without_submission(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation.id.clone(),
        raw_output,
    ));
    let state = state.with_agent_client(client);

    let description = draft_agent_workspace_pr_description(
        &state,
        &conversation,
        &project,
        &workspace,
        &repo,
        &base,
    )
    .await
    .expect("literal tool call output should recover");

    assert_eq!(description.title.as_deref(), Some("Recovered title"));
    assert_eq!(
        description.body_markdown,
        "## Summary\n\nRecovered body & context"
    );
    let stored = state
        .agent_conversation_workspace_repo
        .get_pr_metadata_decision(&conversation.id)
        .await
        .expect("stored description lookup should succeed")
        .expect("recovered description should be stored");
    assert_eq!(
        stored,
        AgentWorkspacePrMetadataDecision::Patch {
            title: Some("Recovered title".to_string()),
            body_markdown: Some(description.body_markdown),
        }
    );
}

#[tokio::test]
async fn invalid_recovered_new_pr_decision_is_not_persisted() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let state = AppState::new_test();
    let raw_output = format!(
        "<call_tool>\n\
         <tool_name>{PR_DESCRIBER_SUBMIT_TOOL}</tool_name>\n\
         <tool_parameters>\n\
         <parameter name=\"conversation_id\">{}</parameter>\n\
         <parameter name=\"decision\">patch</parameter>\n\
         <parameter name=\"title\">Title without a body</parameter>\n\
         </tool_parameters>\n\
         </call_tool>",
        conversation.id
    );
    let client = Arc::new(SubmittingPrDescriptionClient::success_without_submission(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation.id.clone(),
        raw_output,
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
    .expect_err("a new PR decision without a body should fail validation");

    assert!(error
        .to_string()
        .contains("new pull requests require a complete metadata body patch"));
    assert!(
        state
            .agent_conversation_workspace_repo
            .get_pr_metadata_decision(&conversation.id)
            .await
            .expect("stored decision lookup should succeed")
            .is_none(),
        "a target-invalid recovered decision must not remain persisted"
    );
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
async fn draft_pr_description_summarizes_tool_unavailable_output_when_agent_submits_nothing() {
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
    assert!(!error.contains(&raw_output));
}

fn existing_target_from_detail(detail: PrDetail) -> ResolvedAgentWorkspacePrTarget {
    ResolvedAgentWorkspacePrTarget::Existing(Box::new(ExistingPrMetadataSnapshot::from_detail(
        detail,
    )))
}

fn existing_target(body: Option<&str>) -> ResolvedAgentWorkspacePrTarget {
    existing_target_from_detail(PrDetail {
        number: 42,
        title: "Existing title".to_string(),
        body: body.map(str::to_string),
        author: Some("collaborator".to_string()),
        created_at: None,
        url: Some("https://example.test/pr/42".to_string()),
        state: PrStatus::Open,
        is_draft: true,
        head_ref_name: "agent-branch".to_string(),
        base_ref_name: "main".to_string(),
    })
}

#[test]
fn existing_target_cache_authority_changes_for_every_metadata_field() {
    let absent_body = existing_target(None);
    let empty_body = existing_target(Some(""));
    let changed_body = existing_target(Some("changed"));

    assert_ne!(absent_body.cache_authority(), empty_body.cache_authority());
    assert_ne!(empty_body.cache_authority(), changed_body.cache_authority());
    let baseline = PrDetail {
        number: 42,
        title: "Existing title".to_string(),
        body: Some("existing body".to_string()),
        author: Some("collaborator".to_string()),
        created_at: None,
        url: Some("https://example.test/pr/42".to_string()),
        state: PrStatus::Open,
        is_draft: true,
        head_ref_name: "agent-branch".to_string(),
        base_ref_name: "main".to_string(),
    };
    let changes = [
        ("number", {
            let mut detail = baseline.clone();
            detail.number = 43;
            detail
        }),
        ("title", {
            let mut detail = baseline.clone();
            detail.title = "Changed title".to_string();
            detail
        }),
        ("state", {
            let mut detail = baseline.clone();
            detail.state = PrStatus::Closed;
            detail
        }),
        ("draft", {
            let mut detail = baseline.clone();
            detail.is_draft = false;
            detail
        }),
        ("head", {
            let mut detail = baseline.clone();
            detail.head_ref_name = "changed-head".to_string();
            detail
        }),
        ("base", {
            let mut detail = baseline.clone();
            detail.base_ref_name = "changed-base".to_string();
            detail
        }),
    ];
    let baseline_target = existing_target_from_detail(baseline);
    let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
    let baseline_key = AgentWorkspacePrDescriptionCacheKey::for_target(
        conversation_id.clone(),
        "base",
        "head",
        1,
        &baseline_target,
    )
    .unwrap();
    for (field, changed) in changes {
        let changed_target = existing_target_from_detail(changed);
        assert_ne!(
            baseline_target.cache_authority(),
            changed_target.cache_authority(),
            "{field} must invalidate cached metadata decisions"
        );
        assert_ne!(
            baseline_key.cache_key(),
            AgentWorkspacePrDescriptionCacheKey::for_target(
                conversation_id.clone(),
                "base",
                "head",
                1,
                &changed_target,
            )
            .unwrap()
            .cache_key(),
            "{field} must change the typed cache key"
        );
    }
    let key_without_body = AgentWorkspacePrDescriptionCacheKey::for_target(
        ChatConversationId::from_string(uuid::Uuid::new_v4().to_string()),
        "base",
        "head",
        1,
        &absent_body,
    )
    .unwrap();
    let key_with_body = AgentWorkspacePrDescriptionCacheKey::for_target(
        key_without_body.conversation_id.clone(),
        "base",
        "head",
        1,
        &changed_body,
    )
    .unwrap();
    assert_ne!(key_without_body.cache_key(), key_with_body.cache_key());
}

#[test]
fn existing_target_prompt_escapes_untrusted_metadata_and_marks_truncated_body() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let target = existing_target_from_detail(PrDetail {
        number: 42,
        title: "Title <unsafe> & value".to_string(),
        body: Some(format!(
            "Body <unsafe> & {}",
            "x".repeat(MAX_EXISTING_PR_BODY_CONTEXT_CHARS)
        )),
        author: Some("Author <unsafe> & value".to_string()),
        created_at: None,
        url: Some("https://example.test/?a=<unsafe>&b=value".to_string()),
        state: PrStatus::Closed,
        is_draft: true,
        head_ref_name: "head<unsafe>&value".to_string(),
        base_ref_name: "base<unsafe>&value".to_string(),
    });
    let diff_stats = crate::application::git_service::DiffStats {
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        changed_files: Vec::new(),
    };
    let prompt = build_pr_describer_prompt(PrDescriberPromptContext {
        conversation: &conversation,
        project: &project,
        workspace: &workspace,
        effective_cwd: &repo,
        review_base: &base,
        template: &PullRequestTemplateContext {
            source: "workspace",
            content: "## Summary".to_string(),
        },
        commits: &[],
        diff_stats: &diff_stats,
        name_status: "",
        diff_stat: "",
        patch_excerpt: "",
        conversation_context: "",
        target: &target,
    });

    assert!(prompt.contains("<publication_target kind=\"existing_pr\" evidence=\"untrusted\">"));
    assert!(prompt.contains("<author>Author &lt;unsafe&gt; &amp; value</author>"));
    assert!(prompt.contains("<title>Title &lt;unsafe&gt; &amp; value</title>"));
    assert!(prompt.contains(
        "<body complete=\"false\" patch_allowed=\"false\" managed_suffix_preserved=\"false\" \
         max_output_chars=\"60000\">Body &lt;unsafe&gt; &amp;"
    ));
    assert!(prompt.contains("<state>Closed</state>"));
    assert!(prompt.contains("<head_ref>head&lt;unsafe&gt;&amp;value</head_ref>"));
    assert!(prompt.contains("<base_ref>base&lt;unsafe&gt;&amp;value</base_ref>"));
}

#[test]
fn recognized_existing_target_prompt_exposes_only_the_editable_prefix_and_budget() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let preserved_suffix = format!(
        "\n\n{RALPHX_MANAGED_PR_BODY_START}\n{}\n{RALPHX_GENERATED_FOOTER}\n\
         {RALPHX_MANAGED_PR_BODY_END}\n\nOpaque tail",
        "plan".repeat(MAX_EXISTING_PR_BODY_CONTEXT_CHARS)
    );
    let target = existing_target(Some(&format!("Editable prefix{preserved_suffix}")));
    let diff_stats = crate::application::git_service::DiffStats {
        files_changed: 0,
        insertions: 0,
        deletions: 0,
        changed_files: Vec::new(),
    };

    let prompt = build_pr_describer_prompt(PrDescriberPromptContext {
        conversation: &conversation,
        project: &project,
        workspace: &workspace,
        effective_cwd: &repo,
        review_base: &base,
        template: &PullRequestTemplateContext {
            source: "workspace",
            content: "## Summary".to_string(),
        },
        commits: &[],
        diff_stats: &diff_stats,
        name_status: "",
        diff_stat: "",
        patch_excerpt: "",
        conversation_context: "",
        target: &target,
    });

    assert!(prompt.contains(
        "<body complete=\"true\" patch_allowed=\"true\" managed_suffix_preserved=\"true\""
    ));
    assert!(prompt.contains(">Editable prefix</body>"));
    assert!(!prompt.contains("Opaque tail"));
    assert!(!prompt.contains("planplanplan"));
}

#[test]
fn literal_metadata_recovery_accepts_preserve_and_partial_patches_but_rejects_empty_or_malformed() {
    let conversation_id = ChatConversationId::from_string(uuid::Uuid::new_v4().to_string());
    let call = |parameters: &str| {
        format!(
        "<call_tool><tool_name>{PR_DESCRIBER_SUBMIT_TOOL}</tool_name><tool_parameters><parameter name=\"conversation_id\">{conversation_id}</parameter>{parameters}</tool_parameters></call_tool>"
    )
    };
    assert_eq!(
        recover_pr_metadata_decision_from_literal_tool_call(
            &call("<parameter name=\"decision\">preserve</parameter>"),
            &conversation_id
        ),
        Some(AgentWorkspacePrMetadataDecision::Preserve)
    );
    assert_eq!(
        recover_pr_metadata_decision_from_literal_tool_call(&call("<parameter name=\"decision\">patch</parameter><parameter name=\"title\">New title</parameter>"), &conversation_id),
        AgentWorkspacePrMetadataDecision::patch(Some("New title".to_string()), None)
    );
    assert_eq!(
        recover_pr_metadata_decision_from_literal_tool_call(&call("<parameter name=\"decision\">patch</parameter><parameter name=\"body_markdown\">New body</parameter>"), &conversation_id),
        AgentWorkspacePrMetadataDecision::patch(None, Some("New body".to_string()))
    );
    for parameters in [
        "",
        "<parameter name=\"decision\">patch</parameter>",
        "<parameter name=\"decision\">unknown</parameter>",
        "<parameter name=\"decision\">patch</parameter><parameter name=\"title\"> </parameter>",
    ] {
        assert!(recover_pr_metadata_decision_from_literal_tool_call(
            &call(parameters),
            &conversation_id
        )
        .is_none());
    }
}

#[test]
fn target_validation_allows_existing_preserve_and_partial_patch_but_rejects_new_partial() {
    let existing = existing_target(Some("existing body"));
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::Preserve,
        &existing,
    )
    .is_ok());
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::patch(Some("new title".to_string()), None).unwrap(),
        &existing,
    )
    .is_ok());
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::patch(None, Some("body".to_string())).unwrap(),
        &ResolvedAgentWorkspacePrTarget::NewPr,
    )
    .is_ok());
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::patch(Some("only title".to_string()), None).unwrap(),
        &ResolvedAgentWorkspacePrTarget::NewPr,
    )
    .is_err());
}

#[test]
fn existing_target_rejects_body_patch_when_prompt_body_is_truncated() {
    let target = existing_target(Some(&"x".repeat(MAX_EXISTING_PR_BODY_CONTEXT_CHARS + 1)));
    let decision =
        AgentWorkspacePrMetadataDecision::patch(None, Some("replacement".to_string())).unwrap();

    assert!(validate_agent_workspace_pr_metadata_decision(&decision, &target).is_err());
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::Preserve,
        &target,
    )
    .is_ok());
    assert!(validate_agent_workspace_pr_metadata_decision(
        &AgentWorkspacePrMetadataDecision::patch(Some("title only".to_string()), None).unwrap(),
        &target,
    )
    .is_ok());
}

#[test]
fn recognized_managed_suffix_keeps_a_small_editable_prefix_patchable() {
    let managed_suffix = format!(
        "\n\n{RALPHX_MANAGED_PR_BODY_START}\n{}\n{RALPHX_GENERATED_FOOTER}\n\
         {RALPHX_MANAGED_PR_BODY_END}\n\nOpaque tail",
        "plan".repeat(MAX_EXISTING_PR_BODY_CONTEXT_CHARS)
    );
    let target = existing_target(Some(&format!("Small editable prefix{managed_suffix}")));
    let decision =
        AgentWorkspacePrMetadataDecision::patch(None, Some("Replacement".to_string())).unwrap();

    assert!(validate_agent_workspace_pr_metadata_decision(&decision, &target).is_ok());
}

#[test]
fn incomplete_existing_body_decisions_are_constrained_without_losing_a_safe_title() {
    let target = existing_target(Some(&"x".repeat(MAX_EXISTING_PR_BODY_CONTEXT_CHARS + 1)));

    assert_eq!(
        constrain_agent_workspace_pr_metadata_decision(
            AgentWorkspacePrMetadataDecision::patch(
                Some("Improved title".to_string()),
                Some("Unsafe replacement".to_string()),
            )
            .unwrap(),
            &target,
        ),
        AgentWorkspacePrMetadataDecision::patch(Some("Improved title".to_string()), None).unwrap()
    );
    assert_eq!(
        constrain_agent_workspace_pr_metadata_decision(
            AgentWorkspacePrMetadataDecision::patch(None, Some("Unsafe replacement".to_string()))
                .unwrap(),
            &target,
        ),
        AgentWorkspacePrMetadataDecision::Preserve
    );
}

#[test]
fn preserved_suffix_without_editable_budget_downgrades_the_body_field() {
    let remote = format!(
        "Editable\n\n{RALPHX_MANAGED_PR_BODY_START}\n{}\n{RALPHX_GENERATED_FOOTER}\n\
         {RALPHX_MANAGED_PR_BODY_END}",
        "s".repeat(GITHUB_PR_BODY_SOFT_LIMIT_CHARS)
    );
    let target = existing_target(Some(&remote));

    assert_eq!(
        constrain_agent_workspace_pr_metadata_decision(
            AgentWorkspacePrMetadataDecision::patch(
                Some("Improved title".to_string()),
                Some("Improved body".to_string()),
            )
            .unwrap(),
            &target,
        ),
        AgentWorkspacePrMetadataDecision::patch(Some("Improved title".to_string()), None).unwrap()
    );
}

#[test]
fn model_managed_tokens_are_removed_or_downgraded_before_publication() {
    let remote = format!(
        "Editable\n\n{RALPHX_MANAGED_PR_BODY_START}\n{RALPHX_GENERATED_FOOTER}\n\
         {RALPHX_MANAGED_PR_BODY_END}"
    );
    let target = existing_target(Some(&remote));
    let complete_model_copy = format!(
        "Improved editable\n\n{RALPHX_MANAGED_PR_BODY_START}\n{RALPHX_GENERATED_FOOTER}\n\
         {RALPHX_MANAGED_PR_BODY_END}"
    );
    assert_eq!(
        constrain_agent_workspace_pr_metadata_decision(
            AgentWorkspacePrMetadataDecision::patch(None, Some(complete_model_copy)).unwrap(),
            &target,
        ),
        AgentWorkspacePrMetadataDecision::patch(None, Some("Improved editable".to_string()))
            .unwrap()
    );

    let ambiguous = format!("Improved editable\n{RALPHX_MANAGED_PR_BODY_START}");
    assert_eq!(
        constrain_agent_workspace_pr_metadata_decision(
            AgentWorkspacePrMetadataDecision::patch(
                Some("Improved title".to_string()),
                Some(ambiguous),
            )
            .unwrap(),
            &target,
        ),
        AgentWorkspacePrMetadataDecision::patch(Some("Improved title".to_string()), None).unwrap()
    );
}

#[tokio::test]
async fn target_aware_cache_returns_existing_metadata_decision_without_synthesizing_description() {
    let (_temp_dir, repo, base) = create_reviewable_repo();
    let project = project_for(&repo);
    let conversation = conversation_for(&project);
    let workspace = workspace_for(&conversation, &project, &repo, &base);
    let target = existing_target(Some("existing body"));
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    let key = AgentWorkspacePrDescriptionCacheKey::for_target(
        conversation.id.clone(),
        &base,
        head,
        1,
        &target,
    )
    .unwrap();
    invalidate_agent_workspace_pr_description_cache(&conversation.id);
    let raw_output = format!(
        "<call_tool><tool_name>{PR_DESCRIBER_SUBMIT_TOOL}</tool_name><tool_parameters><parameter name=\"conversation_id\">{}</parameter><parameter name=\"decision\">patch</parameter><parameter name=\"title\">Improved title</parameter></tool_parameters></call_tool>",
        conversation.id
    );
    let state = AppState::new_test();
    let client = Arc::new(SubmittingPrDescriptionClient::success_without_submission(
        Arc::clone(&state.agent_conversation_workspace_repo),
        conversation.id.clone(),
        raw_output,
    ));
    let state = state.with_agent_client(client.clone());

    let first = get_or_draft_agent_workspace_pr_metadata_decision(
        &state,
        &conversation,
        &project,
        &workspace,
        &repo,
        &base,
        &target,
        key.clone(),
    )
    .await
    .expect("existing target should return a metadata decision");
    assert_eq!(
        first.cache_status,
        AgentWorkspacePrDescriptionCacheStatus::Miss
    );
    assert_eq!(
        first.decision,
        AgentWorkspacePrMetadataDecision::patch(Some("Improved title".to_string()), None).unwrap()
    );

    let second = get_or_draft_agent_workspace_pr_metadata_decision(
        &state,
        &conversation,
        &project,
        &workspace,
        &repo,
        &base,
        &target,
        key,
    )
    .await
    .expect("existing target should hit the typed cache");
    assert_eq!(
        second.cache_status,
        AgentWorkspacePrDescriptionCacheStatus::Hit
    );
    assert_eq!(second.decision, first.decision);
    assert_eq!(client.spawned_configs().await.len(), 1);
    invalidate_agent_workspace_pr_description_cache(&conversation.id);
}
