use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use super::agent_client_bundle::AgentClientBundle;
use super::agent_workspace_pr_description::{
    DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE, MAX_NAME_STATUS_CHARS, MAX_PATCH_EXCERPT_CHARS,
    MAX_STAT_CHARS,
};
use super::plan_pr_description::{
    build_app_state_plan_pr_description_drafter, build_plan_pr_describer_prompt, read_pr_template,
};
use crate::domain::agents::{
    AgentConfig, AgentHandle, AgentOutput, AgentResponse, AgentResult, AgenticClient,
    ClientCapabilities, ResponseChunk, DEFAULT_AGENT_HARNESS,
};
use crate::domain::entities::{
    AgentWorkspacePrDescription, ArtifactId, ChatContextType, ChatConversationId,
    IdeationSessionId, PlanBranch, Project,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentProviderSettingsRepository,
    ChatConversationRepository,
};
use crate::domain::services::{PlanPrDescriptionDrafter, PrReviewState};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentProviderSettingsRepository,
    MemoryChatConversationRepository,
};
use crate::infrastructure::sqlite::{
    sqlite_agent_conversation_workspace_repo::SqliteAgentConversationWorkspaceRepository,
    sqlite_chat_conversation_repo::SqliteChatConversationRepository,
};
use crate::testing::SqliteTestDb;
use async_trait::async_trait;
use futures::Stream;
use tokio::sync::Mutex;

struct SubmittingPlanPrAgentClient {
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    capabilities: ClientCapabilities,
    last_prompt: Mutex<Option<String>>,
    success: bool,
    submit_description: bool,
}

impl SubmittingPlanPrAgentClient {
    fn new(
        workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
        success: bool,
        submit_description: bool,
    ) -> Self {
        Self {
            workspace_repo,
            capabilities: ClientCapabilities::mock(),
            last_prompt: Mutex::new(None),
            success,
            submit_description,
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
        if self.submit_description {
            let prompt = self.last_prompt().await;
            let conversation_id = ChatConversationId::from_string(
                tag_value(&prompt, "conversation_id")
                    .expect("prompt should include conversation_id"),
            );
            self.workspace_repo
                .save_pr_description(
                    &conversation_id,
                    AgentWorkspacePrDescription::new(
                        None,
                        "## Summary\n\nDrafted by the test describer".to_string(),
                    ),
                )
                .await
                .expect("test client should save submitted description");
        }

        if self.success {
            Ok(AgentOutput::success("completed"))
        } else {
            Ok(AgentOutput::failed("describer failed", 1))
        }
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

fn run_git(repo_path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_plan_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("create temp repo");
    let repo_path = dir.path();

    run_git(repo_path, &["init", "-b", "main"]);
    run_git(repo_path, &["config", "user.email", "test@example.com"]);
    run_git(repo_path, &["config", "user.name", "Test User"]);

    std::fs::create_dir_all(repo_path.join(".github")).expect("create github dir");
    std::fs::write(
        repo_path.join(".github").join("PULL_REQUEST_TEMPLATE.md"),
        "## Custom Summary\n\n- Explain the change",
    )
    .expect("write PR template");
    std::fs::write(repo_path.join("README.md"), "# fixture\n").expect("write readme");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "initial commit"]);

    run_git(
        repo_path,
        &["checkout", "-b", "plan/generated-pr-description"],
    );
    std::fs::create_dir_all(repo_path.join("src")).expect("create src dir");
    std::fs::write(
        repo_path.join("src").join("generated.rs"),
        "pub fn generated_description_fixture() -> &'static str { \"covered\" }\n",
    )
    .expect("write branch file");
    run_git(repo_path, &["add", "."]);
    run_git(repo_path, &["commit", "-m", "Add plan description fixture"]);

    dir
}

fn project_and_plan_branch(repo_path: &Path) -> (Project, PlanBranch) {
    let mut project = Project::new(
        "R&D <Project>".to_string(),
        repo_path.to_string_lossy().into_owned(),
    );
    project.base_branch = Some("main".to_string());

    let mut plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-plan-pr-description"),
        IdeationSessionId::from_string("session-plan-pr-description"),
        project.id.clone(),
        "plan/generated-pr-description".to_string(),
        "main".to_string(),
    );
    plan_branch.pr_eligible = true;

    (project, plan_branch)
}

async fn build_drafter(
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
    workspace_repo: Arc<dyn AgentConversationWorkspaceRepository>,
    agent_client: Arc<dyn AgenticClient>,
) -> Arc<dyn PlanPrDescriptionDrafter> {
    let provider_repo: Arc<dyn AgentProviderSettingsRepository> = Arc::new(
        MemoryAgentProviderSettingsRepository::with_all_providers_enabled(DEFAULT_AGENT_HARNESS),
    );
    let agent_clients = AgentClientBundle::from_default_client(DEFAULT_AGENT_HARNESS, agent_client);
    build_app_state_plan_pr_description_drafter(
        chat_conversation_repo,
        workspace_repo,
        provider_repo,
        agent_clients,
    )
}

#[tokio::test]
async fn draft_plan_description_runs_describer_and_cleans_synthetic_workspace() {
    let repo = setup_plan_repo();
    let (project, plan_branch) = project_and_plan_branch(repo.path());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let chat_conversation_repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let client = Arc::new(SubmittingPlanPrAgentClient::new(
        Arc::clone(&workspace_repo_trait),
        true,
        true,
    ));
    let client_trait: Arc<dyn AgenticClient> = client.clone();
    let drafter = build_drafter(
        Arc::clone(&chat_conversation_repo),
        Arc::clone(&workspace_repo_trait),
        client_trait,
    )
    .await;

    let body = drafter
        .draft_plan_description(&project, &plan_branch, "main", PrReviewState::Draft)
        .await
        .expect("plan PR describer should return submitted description");

    assert_eq!(
        body.body_markdown,
        "## Summary\n\nDrafted by the test describer"
    );
    let prompt = client.last_prompt().await;
    assert!(prompt.contains("submit_agent_workspace_pr_description exactly once"));
    assert!(prompt.contains("<project_name>R&amp;D &lt;Project&gt;</project_name>"));
    assert!(prompt.contains("<branch_name>plan/generated-pr-description</branch_name>"));
    assert!(prompt.contains("<review_state>draft</review_state>"));
    assert!(prompt.contains("src/generated.rs"));
    assert!(prompt.contains("Add plan description fixture"));
    assert!(prompt.contains("## Custom Summary"));
    assert!(
        workspace_repo
            .get_by_project_id(&project.id)
            .await
            .expect("workspace lookup should succeed")
            .is_empty(),
        "synthetic workspace should be removed after drafting"
    );
    assert!(
        chat_conversation_repo
            .get_by_context(ChatContextType::Project, project.id.as_str())
            .await
            .expect("conversation lookup should succeed")
            .is_empty(),
        "synthetic conversation should be removed after drafting"
    );
}

#[tokio::test]
async fn draft_plan_description_sqlite_creates_parent_for_synthetic_workspace() {
    let repo = setup_plan_repo();
    let (project, plan_branch) = project_and_plan_branch(repo.path());
    let db = SqliteTestDb::new("plan_pr_description_sqlite");
    let workspace_repo = Arc::new(SqliteAgentConversationWorkspaceRepository::from_shared(
        db.shared_conn(),
    ));
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let chat_conversation_repo = Arc::new(SqliteChatConversationRepository::from_shared(
        db.shared_conn(),
    ));
    let chat_conversation_repo_trait: Arc<dyn ChatConversationRepository> =
        chat_conversation_repo.clone();
    let client = Arc::new(SubmittingPlanPrAgentClient::new(
        Arc::clone(&workspace_repo_trait),
        true,
        true,
    ));
    let client_trait: Arc<dyn AgenticClient> = client.clone();
    let drafter = build_drafter(
        Arc::clone(&chat_conversation_repo_trait),
        Arc::clone(&workspace_repo_trait),
        client_trait,
    )
    .await;

    let body = drafter
        .draft_plan_description(&project, &plan_branch, "main", PrReviewState::Ready)
        .await
        .expect("SQLite-backed plan PR describer should create a valid parent conversation");

    assert_eq!(
        body.body_markdown,
        "## Summary\n\nDrafted by the test describer"
    );
    let prompt = client.last_prompt().await;
    let synthetic_id =
        tag_value(&prompt, "conversation_id").expect("prompt should include conversation_id");
    let (conversation_count, workspace_count) = db.with_connection(|conn| {
        let conversation_count = conn
            .query_row(
                "SELECT COUNT(*) FROM chat_conversations WHERE id = ?1",
                rusqlite::params![synthetic_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("conversation count query should succeed");
        let workspace_count = conn
            .query_row(
                "SELECT COUNT(*) FROM agent_conversation_workspaces WHERE conversation_id = ?1",
                rusqlite::params![synthetic_id.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .expect("workspace count query should succeed");
        (conversation_count, workspace_count)
    });
    assert_eq!(conversation_count, 0, "synthetic conversation is cleaned");
    assert_eq!(workspace_count, 0, "synthetic workspace is cleaned");
}

#[tokio::test]
async fn draft_plan_description_returns_error_when_agent_fails() {
    let repo = setup_plan_repo();
    let (project, plan_branch) = project_and_plan_branch(repo.path());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let chat_conversation_repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let client = Arc::new(SubmittingPlanPrAgentClient::new(
        Arc::clone(&workspace_repo_trait),
        false,
        false,
    ));
    let client_trait: Arc<dyn AgenticClient> = client;
    let drafter = build_drafter(
        Arc::clone(&chat_conversation_repo),
        Arc::clone(&workspace_repo_trait),
        client_trait,
    )
    .await;

    let body = drafter
        .draft_plan_description(&project, &plan_branch, "main", PrReviewState::Ready)
        .await;

    assert!(body.is_err());
    assert!(
        workspace_repo
            .get_by_project_id(&project.id)
            .await
            .expect("workspace lookup should succeed")
            .is_empty(),
        "synthetic workspace should still be cleaned on failure"
    );
    assert!(
        chat_conversation_repo
            .get_by_context(ChatContextType::Project, project.id.as_str())
            .await
            .expect("conversation lookup should succeed")
            .is_empty(),
        "synthetic conversation should still be cleaned on failure"
    );
}

#[tokio::test]
async fn draft_plan_description_returns_error_when_agent_submits_no_description() {
    let repo = setup_plan_repo();
    let (project, plan_branch) = project_and_plan_branch(repo.path());
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let workspace_repo_trait: Arc<dyn AgentConversationWorkspaceRepository> =
        workspace_repo.clone();
    let chat_conversation_repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let client = Arc::new(SubmittingPlanPrAgentClient::new(
        Arc::clone(&workspace_repo_trait),
        true,
        false,
    ));
    let client_trait: Arc<dyn AgenticClient> = client;
    let drafter = build_drafter(
        Arc::clone(&chat_conversation_repo),
        Arc::clone(&workspace_repo_trait),
        client_trait,
    )
    .await;

    let body = drafter
        .draft_plan_description(&project, &plan_branch, "main", PrReviewState::Ready)
        .await;

    assert!(body.is_err());
    assert!(
        workspace_repo
            .get_by_project_id(&project.id)
            .await
            .expect("workspace lookup should succeed")
            .is_empty(),
        "synthetic workspace should still be cleaned without a submitted description"
    );
    assert!(
        chat_conversation_repo
            .get_by_context(ChatContextType::Project, project.id.as_str())
            .await
            .expect("conversation lookup should succeed")
            .is_empty(),
        "synthetic conversation should still be cleaned without a submitted description"
    );
}

#[tokio::test]
async fn read_pr_template_uses_custom_template_or_default_fallback() {
    let dir = tempfile::tempdir().expect("create temp dir");

    let fallback = read_pr_template(dir.path()).await;
    assert_eq!(
        fallback,
        DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.trim().to_string()
    );

    std::fs::create_dir_all(dir.path().join(".github")).expect("create github dir");
    std::fs::write(
        dir.path().join(".github").join("PULL_REQUEST_TEMPLATE.md"),
        "\n\n## Custom\n\n- Body\n\n",
    )
    .expect("write template");

    let custom = read_pr_template(dir.path()).await;
    assert_eq!(custom, "## Custom\n\n- Body");

    std::fs::write(
        dir.path().join(".github").join("PULL_REQUEST_TEMPLATE.md"),
        "   \n",
    )
    .expect("write empty template");
    let empty_fallback = read_pr_template(dir.path()).await;
    assert_eq!(
        empty_fallback,
        DEFAULT_AGENT_WORKSPACE_PR_TEMPLATE.trim().to_string()
    );
}

#[test]
fn build_plan_pr_describer_prompt_escapes_and_truncates_context() {
    let project = Project::new("A&B <Project>".to_string(), "/tmp/a&b".to_string());
    let plan_branch = PlanBranch::new(
        ArtifactId::from_string("artifact-1"),
        IdeationSessionId::from_string("session-1"),
        project.id.clone(),
        "plan/<feature>&branch".to_string(),
        "main&base".to_string(),
    );
    let conversation_id =
        ChatConversationId::from_string("11111111-1111-1111-1111-111111111111".to_string());
    let commits = vec![crate::application::git_service::CommitInfo {
        sha: "abc".repeat(14),
        short_sha: "abc1234".to_string(),
        message: "Add <unsafe> & useful change".to_string(),
        author: "A&B".to_string(),
        timestamp: "2026-06-17T00:00:00Z".to_string(),
    }];
    let diff_stats = crate::application::git_service::DiffStats {
        files_changed: 1,
        insertions: 2,
        deletions: 3,
        changed_files: vec!["src/<unsafe>&file.rs".to_string()],
    };
    let long_name_status = "N".repeat(MAX_NAME_STATUS_CHARS + 17);
    let long_diff_stat = "S".repeat(MAX_STAT_CHARS + 17);
    let long_patch = "P".repeat(MAX_PATCH_EXCERPT_CHARS + 17);

    let prompt = build_plan_pr_describer_prompt(
        &conversation_id,
        &project,
        &plan_branch,
        "main&base",
        PrReviewState::Ready,
        "## Template & <Heading>",
        &commits,
        &diff_stats,
        &long_name_status,
        &long_diff_stat,
        &long_patch,
    );

    assert!(prompt.contains("<project_name>A&amp;B &lt;Project&gt;</project_name>"));
    assert!(prompt.contains("<registered_project_cwd>/tmp/a&amp;b</registered_project_cwd>"));
    assert!(prompt.contains("<base_ref>main&amp;base</base_ref>"));
    assert!(prompt.contains("<branch_name>plan/&lt;feature&gt;&amp;branch</branch_name>"));
    assert!(prompt.contains("<review_state>ready</review_state>"));
    assert!(prompt.contains("## Template &amp; &lt;Heading&gt;"));
    assert!(prompt.contains("src/&lt;unsafe&gt;&amp;file.rs"));
    assert!(prompt.contains("Add &lt;unsafe&gt; &amp; useful change"));
    assert_eq!(
        tag_value(&prompt, "name_status")
            .expect("name_status should be present")
            .chars()
            .count(),
        MAX_NAME_STATUS_CHARS
    );
    assert_eq!(
        tag_value(&prompt, "diff_stat")
            .expect("diff_stat should be present")
            .chars()
            .count(),
        MAX_STAT_CHARS
    );
    assert_eq!(
        tag_value(&prompt, "patch_excerpt")
            .expect("patch_excerpt should be present")
            .chars()
            .count(),
        MAX_PATCH_EXCERPT_CHARS
    );
}
