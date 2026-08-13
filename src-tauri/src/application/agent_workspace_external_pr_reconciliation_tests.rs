use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use async_trait::async_trait;
use ralphx_events::NullEventSink;

use crate::application::agent_workspace_external_pr_reconciliation::{
    external_pr_reconciliation_skip_reason, reconcile_agent_workspace_external_pr,
    reconcile_recent_agent_workspace_external_prs_on_startup,
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps,
    AgentWorkspaceExternalPrReconciliationDeps, AgentWorkspaceExternalPrReconciliationOutcome,
    AgentWorkspaceExternalPrReconciliationTrigger,
};
use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::clickup_integration_service::{
    ClickUpApiClient, ClickUpAuthContext, ClickUpIntegrationService, ClickUpTaskContent,
    ClickUpWorkspace,
};
use crate::application::external_issue_link_service::ExternalIssueLinkService;
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    AgentRun, AgentWorkspaceRepairPhase, ChatConversationId, IdeationAnalysisBaseRefKind,
    PlanBranchId, Project,
};
use crate::domain::integrations::{
    ClickUpIntegrationSettings, ClickUpIntegrationSettingsRepository, IntegrationValidationStatus,
};
use crate::domain::repositories::{
    AgentConversationWorkspaceRepository, AgentRunRepository, AgentWorkspaceRepairRepository,
    BranchUpdateRepository, ProjectRepository,
};
use crate::domain::services::github_service::{
    PrDetail, PrHealth, PrMergeStateStatus, PrMergeableState, PrSyncState,
};
use crate::domain::services::{GithubServiceTrait, PrBranchMatch, PrStatus, SecretStore};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryBranchUpdateRepository, MemoryClickUpIntegrationSettingsRepository,
    MemoryExternalIssueLinkRepository, MemoryPlanBranchRepository, MemoryProjectRepository,
    MemorySecretStore,
};
use crate::tests::mock_github_service::MockGithubService;

struct StaticClickUpClient {
    task: ClickUpTaskContent,
}

#[async_trait]
impl ClickUpApiClient for StaticClickUpClient {
    async fn validate(&self, _auth: &ClickUpAuthContext) -> Result<(), String> {
        Ok(())
    }

    async fn list_workspaces(
        &self,
        _auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        Ok(Vec::new())
    }

    async fn fetch_task(
        &self,
        _auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        if self.task.id.eq_ignore_ascii_case(task_id)
            || self
                .task
                .custom_id
                .as_deref()
                .is_some_and(|custom_id| custom_id.eq_ignore_ascii_case(task_id))
        {
            Ok(self.task.clone())
        } else {
            Err("HTTP 404: task not found".to_string())
        }
    }

    async fn fetch_task_by_custom_id(
        &self,
        auth: &ClickUpAuthContext,
        _team_id: &str,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        self.fetch_task(auth, task_id).await
    }
}

fn test_project() -> Project {
    let mut project = Project::new("Demo".to_string(), "/tmp/ralphx-demo".to_string());
    project.base_branch = Some("main".to_string());
    project.github_pr_enabled = true;
    project
}

fn test_workspace(project: &Project) -> AgentConversationWorkspace {
    test_workspace_with_id(project, "11111111-1111-1111-1111-111111111111")
}

fn test_workspace_with_id(project: &Project, id: &str) -> AgentConversationWorkspace {
    let conversation_id = ChatConversationId::from_string(id.to_string());
    AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        format!("ralphx/demo/agent-{id}"),
        PathBuf::from(format!("/tmp/ralphx-demo-worktree-{id}"))
            .to_string_lossy()
            .to_string(),
    )
}

fn clickup_task(id: &str, custom_id: &str) -> ClickUpTaskContent {
    ClickUpTaskContent {
        id: id.to_string(),
        custom_id: Some(custom_id.to_string()),
        name: "Validated ClickUp task".to_string(),
        url: Some(format!("https://app.clickup.com/t/{id}")),
        description: String::new(),
        status_name: None,
        status_type: None,
        status_category: None,
        creator: None,
        assignees: Vec::new(),
        watchers: Vec::new(),
        tags: Vec::new(),
        comments: Vec::new(),
        attachments: Vec::new(),
        updated_at: None,
        space_id: None,
        list_name: None,
    }
}

async fn clickup_service(task: ClickUpTaskContent) -> ClickUpIntegrationService {
    let settings = Arc::new(MemoryClickUpIntegrationSettingsRepository::new());
    let secrets = Arc::new(MemorySecretStore::new());
    secrets
        .put_secret("clickup-test-token", "pk_test")
        .await
        .expect("secret should save");
    settings
        .upsert(&ClickUpIntegrationSettings {
            enabled: true,
            token_secret_ref: Some("clickup-test-token".to_string()),
            workspace_id: Some("workspace-1".to_string()),
            validation_status: IntegrationValidationStatus::Valid,
            task_search_available: true,
            ..Default::default()
        })
        .await
        .expect("settings should save");
    ClickUpIntegrationService::new(settings, secrets, Arc::new(StaticClickUpClient { task }))
}

fn pr_detail(number: i64, branch: &str, title: &str, body: Option<&str>) -> PrDetail {
    PrDetail {
        number,
        title: title.to_string(),
        url: Some(format!("https://github.com/owner/repo/pull/{number}")),
        head_ref_name: branch.to_string(),
        base_ref_name: "main".to_string(),
        body: body.map(str::to_string),
        is_draft: false,
        state: PrStatus::Open,
        author: None,
        created_at: None,
    }
}

async fn wait_for_latest_pr_lookup_calls(github: &MockGithubService, expected: u32) {
    for _ in 0..100 {
        if github.state().find_latest_pr_by_head_branch_calls >= expected {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!(
        "expected at least {expected} latest PR lookups, got {}",
        github.state().find_latest_pr_by_head_branch_calls
    );
}

async fn deps_with_workspace(
    project: Project,
    workspace: AgentConversationWorkspace,
    github: Arc<MockGithubService>,
) -> (
    AgentWorkspaceExternalPrReconciliationDeps,
    Arc<MemoryAgentConversationWorkspaceRepository>,
) {
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should save");
    if let Some(pr_number) = workspace.publication_pr_number {
        github.will_return_pr_detail(pr_detail(
            pr_number,
            &workspace.branch_name,
            "Owned workspace pull request",
            None,
        ));
    }

    (
        AgentWorkspaceExternalPrReconciliationDeps {
            workspace_repo: workspace_repo.clone(),
            chat_conversation_repo: Arc::new(
                crate::infrastructure::memory::MemoryChatConversationRepository::new(),
            ),
            project_repo,
            github,
            clickup_integration_service: None,
            external_issue_link_service: None,
            pr_poller_registry: None,
            chat_service: None,
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
            agent_workspace_repair_repo: None,
            plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
            events: Arc::new(NullEventSink),
            durable_recovery_state: None,
        },
        workspace_repo,
    )
}

#[tokio::test]
async fn reconciliation_links_external_open_pr_to_unpublished_workspace() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 42,
        url: "https://github.com/owner/repo/pull/42".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:00:00Z".to_string()),
        author_login: None,
    })));
    let (mut deps, workspace_repo) =
        deps_with_workspace(project, workspace.clone(), github.clone()).await;
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    deps.pr_poller_registry = Some(Arc::clone(&registry));
    deps.chat_service = Some(Arc::new(MockChatService::new()) as Arc<dyn ChatService>);
    deps.agent_workspace_repair_repo = Some(workspace_repo.clone());

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 42,
            pr_status: "open".to_string()
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, Some(42));
    assert_eq!(
        updated.publication_pr_url.as_deref(),
        Some("https://github.com/owner/repo/pull/42")
    );
    assert_eq!(updated.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);

    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "external_pr_linked");
    assert!(registry.is_agent_workspace_polling(&conversation_id));
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn reconciliation_restarts_polling_for_linked_open_pr() {
    let project = test_project();
    let mut workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    workspace.publication_pr_number = Some(42);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/42".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    let github = Arc::new(MockGithubService::new());
    github.will_return_status(PrStatus::Open);
    let (mut deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    deps.pr_poller_registry = Some(Arc::clone(&registry));
    deps.chat_service = Some(Arc::new(MockChatService::new()) as Arc<dyn ChatService>);
    deps.agent_workspace_repair_repo = Some(workspace_repo);

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("linked_pr_not_terminal")
    );
    assert!(
        registry.is_agent_workspace_polling(&conversation_id),
        "a linked open PR must regain CI supervision after a poller is lost"
    );
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn live_reconciliation_fails_closed_without_a_durable_repair_repository() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 42,
        url: "https://github.com/owner/repo/pull/42".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:00:00Z".to_string()),
        author_login: None,
    })));
    let (mut deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    let chat = Arc::new(MockChatService::new());
    deps.pr_poller_registry = Some(Arc::clone(&registry));
    deps.chat_service = Some(chat.clone() as Arc<dyn ChatService>);

    let error = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect_err("live reconciliation must not fall back to the legacy poller");

    assert!(error
        .to_string()
        .contains("durable workspace repair repository"));
    assert!(chat.get_sent_messages().await.is_empty());
    assert!(!registry.is_agent_workspace_polling(&conversation_id));
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);
}

#[tokio::test]
async fn live_reconciliation_routes_pr_conflicts_through_one_durable_repair_attempt() {
    let git_root = tempfile::tempdir_in(std::env::current_dir().expect("current directory"))
        .expect("git root");
    for args in [
        vec!["init", "-b", "ralphx/demo/agent-conflict"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "RalphX Test"],
        vec!["commit", "--allow-empty", "-m", "base"],
    ] {
        let output = std::process::Command::new("git")
            .args(&args)
            .current_dir(git_root.path())
            .output()
            .expect("git fixture command should spawn");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut project = test_project();
    project.working_directory = git_root.path().to_string_lossy().to_string();
    let mut workspace = test_workspace(&project);
    workspace.branch_name = "ralphx/demo/agent-conflict".to_string();
    workspace.worktree_path = git_root.path().to_string_lossy().to_string();
    workspace.auto_publish_enabled = true;
    workspace.pr_auto_merge_desired = false;
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 42,
        url: "https://github.com/owner/repo/pull/42".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:00:00Z".to_string()),
        author_login: None,
    })));
    let conflict_health = PrHealth {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Dirty),
            mergeable: Some(PrMergeableState::Conflicting),
            is_draft: false,
            head_ref_name: workspace.branch_name.clone(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("conflict-head".to_string()),
            base_ref_oid: Some("base-sha".to_string()),
        },
        review_decision: None,
        checks: Vec::new(),
        issue_comments: Vec::new(),
        auto_merge_request: None,
    };
    github.state().fetch_pr_health_result = Some(Ok(conflict_health.clone()));

    let (mut deps, workspace_repo) =
        deps_with_workspace(project, workspace.clone(), github.clone()).await;
    let repair_repo: Arc<dyn AgentWorkspaceRepairRepository> = workspace_repo.clone();
    let registry = Arc::new(PrPollerRegistry::new(
        Some(Arc::clone(&github) as Arc<dyn GithubServiceTrait>),
        Arc::new(MemoryPlanBranchRepository::new()),
    ));
    registry.set_branch_update_repo(
        Arc::new(MemoryBranchUpdateRepository::new()) as Arc<dyn BranchUpdateRepository>
    );
    let chat = Arc::new(MockChatService::new());
    deps.pr_poller_registry = Some(Arc::clone(&registry));
    deps.chat_service = Some(chat.clone() as Arc<dyn ChatService>);
    deps.agent_workspace_repair_repo = Some(Arc::clone(&repair_repo));

    reconcile_agent_workspace_external_pr(
        deps.clone(),
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("initial live reconciliation should link the external PR");

    let mut first_attempt = None;
    for _ in 0..100 {
        if let Some(attempt) = repair_repo
            .get_current_repair_attempt(&conversation_id)
            .await
            .expect("durable repair lookup should succeed")
        {
            if attempt.phase == AgentWorkspaceRepairPhase::Repairing
                && chat.get_sent_messages().await.len() == 1
            {
                first_attempt = Some(attempt);
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let first_attempt = first_attempt.expect("durable repair attempt should be created");
    assert_eq!(first_attempt.generation, 1);
    assert_eq!(
        first_attempt.target_base_commit.as_deref(),
        Some("base-sha")
    );
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    let events_before_repeat = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list");
    for _ in 0..100 {
        if !registry.is_agent_workspace_polling(&conversation_id) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        !registry.is_agent_workspace_polling(&conversation_id),
        "the initial conflict poller should settle before the repeat signal"
    );

    let mut stale_workspace = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    stale_workspace.base_commit = Some("stale-observation-base".to_string());
    workspace_repo
        .create_or_update(stale_workspace)
        .await
        .expect("stale workspace observation should save");
    github.state().fetch_pr_health_result = Some(Ok(conflict_health.clone()));

    reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
    )
    .await
    .expect("repeat live reconciliation should stay durable");

    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(
        github.state().fetch_pr_status_snapshots_calls.len(),
        1,
        "an unsettled durable repair keeps the repeat poller from re-reading or re-dispatching"
    );

    let current_attempt = repair_repo
        .get_current_repair_attempt(&conversation_id)
        .await
        .expect("durable repair lookup should succeed")
        .expect("repair attempt should remain active");
    assert_eq!(current_attempt.id, first_attempt.id);
    assert_eq!(current_attempt.generation, 1);
    assert_eq!(
        current_attempt.target_base_commit.as_deref(),
        Some("base-sha")
    );
    assert_eq!(chat.get_sent_messages().await.len(), 1);
    assert_eq!(
        workspace_repo
            .list_publication_events(&conversation_id)
            .await
            .expect("events should list"),
        events_before_repeat
    );
    assert!(repair_repo
        .get_open_repair_effect(&first_attempt.id)
        .await
        .expect("repair effects should load")
        .is_none());
    let github_state = github.state();
    assert_eq!(github_state.disable_pr_auto_merge_calls, 0);
    assert_eq!(github_state.push_branch_calls, 0);
    assert_eq!(
        github_state.push_branch_with_expected_remote_oid_lease_calls,
        0
    );
    drop(github_state);
    registry.stop_agent_workspace_polling(&conversation_id);
}

#[tokio::test]
async fn reconciliation_marks_external_merged_pr_terminal() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 43,
        url: "https://github.com/owner/repo/pull/43".to_string(),
        status: PrStatus::Merged {
            merge_commit_sha: Some("merge-sha".to_string()),
            merged_at: None,
        },
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:05:00Z".to_string()),
        author_login: None,
    })));
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::Startup,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 43,
            pr_status: "merged".to_string()
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, Some(43));
    assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));

    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "external_pr_merged");
}

#[tokio::test]
async fn reconciliation_reports_terminal_runtime_shutdown_failure() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 143,
        url: "https://github.com/owner/repo/pull/143".to_string(),
        status: PrStatus::Merged {
            merge_commit_sha: Some("merge-sha".to_string()),
            merged_at: None,
        },
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:05:00Z".to_string()),
        author_login: None,
    })));
    let (mut deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    let agent_run_repo = Arc::new(MemoryAgentRunRepository::new());
    let run = agent_run_repo
        .create(AgentRun::new(conversation_id.clone()))
        .await
        .expect("active run should persist");
    deps.agent_run_repo = agent_run_repo.clone();

    let error = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::Startup,
    )
    .await
    .expect_err("missing chat runtime must block terminal reconciliation success");

    assert!(error.to_string().contains("no chat runtime was available"));
    assert!(agent_run_repo
        .get_by_id(&run.id)
        .await
        .expect("run lookup")
        .expect("run retained")
        .is_active());
    let persisted = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup")
        .expect("workspace retained");
    assert_eq!(persisted.publication_pr_status.as_deref(), Some("merged"));
    assert!(workspace_repo
        .get_local_cleanup_status(&conversation_id)
        .await
        .expect("cleanup status lookup")
        .is_none());
}

#[tokio::test]
async fn reconciliation_links_external_draft_pr() {
    let project = test_project();
    let workspace = test_workspace_with_id(&project, "22222222-2222-2222-2222-222222222222");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 44,
        url: "https://github.com/owner/repo/pull/44".to_string(),
        status: PrStatus::Open,
        is_draft: true,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:10:00Z".to_string()),
        author_login: None,
    })));
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 44,
            pr_status: "draft".to_string()
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("draft"));
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events[0].step, "external_pr_linked");
}

#[tokio::test]
async fn reconciliation_links_clickup_ticket_from_external_pr_evidence() {
    let project = test_project();
    let mut workspace = test_workspace_with_id(&project, "2c2c2c2c-2c2c-2c2c-2c2c-2c2c2c2c2c2c");
    // PR title/body are discovery-only evidence; the link itself must be
    // authorized by a workspace-owned signal, so the ticket token lives in
    // the branch name.
    workspace.branch_name = "feature/DEV-42-link-clickup".to_string();
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 144,
        url: "https://github.com/owner/repo/pull/144".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:12:00Z".to_string()),
        author_login: None,
    })));
    github.will_return_pr_detail(pr_detail(
        144,
        &workspace.branch_name,
        "DEV-42 link ClickUp ticket",
        Some("Implements the requested ClickUp task."),
    ));
    let links = Arc::new(ExternalIssueLinkService::new(Arc::new(
        MemoryExternalIssueLinkRepository::new(),
    )));
    let (mut deps, _workspace_repo) =
        deps_with_workspace(project.clone(), workspace.clone(), github.clone()).await;
    deps.clickup_integration_service = Some(Arc::new(
        clickup_service(clickup_task("8689abc", "DEV-42")).await,
    ));
    deps.external_issue_link_service = Some(Arc::clone(&links));

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 144,
            pr_status: "open".to_string()
        }
    );
    assert_eq!(github.state().fetch_pr_detail_calls, 1);
    let ticket_links = links
        .list_ticket_links_for_conversation(&conversation_id.as_str())
        .await
        .expect("ticket links should load");
    assert_eq!(ticket_links.len(), 1);
    assert_eq!(ticket_links[0].provider, "clickup");
    assert_eq!(ticket_links[0].external_id, "8689abc");
    assert_eq!(ticket_links[0].external_key.as_deref(), Some("DEV-42"));
    assert_eq!(
        ticket_links[0].external_url.as_deref(),
        Some("https://app.clickup.com/t/8689abc")
    );
    assert_eq!(
        ticket_links[0].local_project_id.as_deref(),
        Some(project.id.as_str())
    );
    let sync_records = links
        .list_sync_records_for_link(&ticket_links[0].id)
        .await
        .expect("sync records should load");
    assert_eq!(sync_records.len(), 1);
    assert_eq!(sync_records[0].sync_kind, "clickup_git_association");
    assert_eq!(sync_records[0].local_state.as_deref(), Some("open"));
}

#[tokio::test]
async fn reconciliation_marks_external_closed_pr_terminal_without_fetch() {
    let project = test_project();
    let workspace = test_workspace_with_id(&project, "33333333-3333-3333-3333-333333333333");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 45,
        url: "https://github.com/owner/repo/pull/45".to_string(),
        status: PrStatus::Closed,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:15:00Z".to_string()),
        author_login: None,
    })));
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::Startup,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 45,
            pr_status: "closed".to_string()
        }
    );
    assert_eq!(github.state().fetch_remote_calls, 0);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events[0].step, "external_pr_closed");
}

#[tokio::test]
async fn reconciliation_keeps_workspace_unchanged_when_no_external_pr_matches() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::NotFound
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_status, None);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);
}

#[tokio::test]
async fn reconciliation_rejects_external_pr_with_a_different_head_branch() {
    let project = test_project();
    let workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 942,
        url: "https://github.com/owner/repo/pull/942".to_string(),
        status: PrStatus::Open,
        is_draft: false,
        head_ref_name: "another-agent-branch".to_string(),
        updated_at: Some("2026-05-11T22:00:00Z".to_string()),
        author_login: None,
    })));
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should reject the mismatched PR safely");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::NotFound
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .expect("workspace lookup should succeed")
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_status, None);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .expect("events should list")
        .is_empty());
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);
}

fn archived_workspace_with_publication(project: &Project) -> AgentConversationWorkspace {
    let mut workspace = test_workspace(project);
    workspace.status = AgentConversationWorkspaceStatus::Archived;
    workspace.publication_pr_number = Some(913);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/913".to_string());
    workspace.publication_pr_status = Some("merged".to_string());
    workspace
}

#[tokio::test]
async fn reconciliation_corrects_foreign_publication_on_archived_workspace() {
    let project = test_project();
    let workspace = archived_workspace_with_publication(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    github.will_return_pr_detail(pr_detail(
        913,
        "another-team-branch",
        "Foreign pull request",
        None,
    ));

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("archived foreign publication should be correctable");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("foreign_publication_corrected")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    assert_eq!(updated.publication_pr_status, None);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "publication_association_corrected");
    assert_eq!(github.state().check_pr_status_calls, 0);
}

#[tokio::test]
async fn reconciliation_reports_unverified_archived_foreign_publication_on_pr_detail_error() {
    let project = test_project();
    let workspace = archived_workspace_with_publication(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    github.will_fail_pr_detail("gh unavailable");

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("an unreadable PR detail should degrade to unverified");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("foreign_publication_unverified")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, Some(913));
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn reconciliation_keeps_archived_workspace_with_owned_publication_untouched() {
    let project = test_project();
    let workspace = archived_workspace_with_publication(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    // deps_with_workspace configures the owned PR detail: the PR head matches the
    // workspace branch, so the correction must classify it as owned and skip.
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("owned archived publication should stay skipped");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("workspace_not_active")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, Some(913));
    assert_eq!(updated.status, AgentConversationWorkspaceStatus::Archived);
    assert!(workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap()
        .is_empty());
    assert_eq!(github.state().check_pr_status_calls, 0);
}

#[tokio::test]
async fn reconciliation_skips_archived_foreign_publication_when_project_is_missing() {
    let stored_project = test_project();
    let missing_project = Project::new(
        "Missing".to_string(),
        "/tmp/ralphx-missing-project".to_string(),
    );
    let workspace = archived_workspace_with_publication(&missing_project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, _workspace_repo) =
        deps_with_workspace(stored_project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id,
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("missing project should skip the correction");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("project_missing")
    );
    assert_eq!(github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn reconciliation_skips_archived_foreign_publication_when_project_is_archived() {
    let mut project = test_project();
    project.archived_at = Some(chrono::Utc::now());
    let workspace = archived_workspace_with_publication(&project);
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, _workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id,
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("archived project should skip the correction");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("project_archived")
    );
    assert_eq!(github.state().fetch_pr_detail_calls, 0);
}

#[tokio::test]
async fn reconciliation_corrects_foreign_publication_on_active_linked_workspace() {
    let project = test_project();
    let mut workspace = test_workspace(&project);
    workspace.publication_pr_number = Some(99);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/99".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;
    github.will_return_pr_detail(pr_detail(
        99,
        "another-team-branch",
        "Foreign pull request",
        None,
    ));

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("linked foreign publication should be correctable");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("foreign_publication_corrected")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_number, None);
    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "publication_association_corrected");
    assert_eq!(github.state().check_pr_status_calls, 0);
}

#[tokio::test]
async fn reconciliation_leaves_linked_open_pr_repair_state_unchanged() {
    let project = test_project();
    let mut workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    workspace.publication_pr_number = Some(99);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    let github = Arc::new(MockGithubService::new());
    github.will_return_status(PrStatus::Open);
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("linked_pr_not_terminal")
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("open"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("failed"));
    assert_eq!(updated.pr_supervision_status.as_deref(), Some("blocked"));
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
    assert_eq!(github.state().check_pr_status_calls, 1);
}

#[tokio::test]
async fn reconciliation_marks_linked_merged_pr_terminal_even_when_workspace_missing() {
    let project = test_project();
    let mut workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    workspace.status = AgentConversationWorkspaceStatus::Missing;
    workspace.publication_pr_number = Some(263);
    workspace.publication_pr_url = Some("https://github.com/owner/repo/pull/263".to_string());
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("needs_agent".to_string());
    workspace.pr_supervision_status = Some("monitoring".to_string());
    workspace.pr_supervision_summary = Some("RalphX is monitoring the pull request.".to_string());
    let github = Arc::new(MockGithubService::new());
    github.will_return_status(PrStatus::Merged {
        merge_commit_sha: Some("merge-sha".to_string()),
        merged_at: None,
    });
    let (deps, workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Linked {
            pr_number: 263,
            pr_status: "merged".to_string()
        }
    );
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("workspace should exist");
    assert_eq!(updated.status, AgentConversationWorkspaceStatus::Missing);
    assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status, None);
    assert_eq!(updated.pr_supervision_summary, None);
    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);

    let events = workspace_repo
        .list_publication_events(&conversation_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].step, "pr_merged");
}

#[tokio::test]
async fn reconciliation_skips_missing_workspace_project_and_disabled_projects() {
    let project = test_project();
    let workspace = test_workspace_with_id(&project, "44444444-4444-4444-4444-444444444444");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());

    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![]));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    let deps = AgentWorkspaceExternalPrReconciliationDeps {
        workspace_repo: workspace_repo.clone(),
        chat_conversation_repo: Arc::new(
            crate::infrastructure::memory::MemoryChatConversationRepository::new(),
        ),
        project_repo: project_repo.clone(),
        github: github.clone(),
        clickup_integration_service: None,
        external_issue_link_service: None,
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        agent_workspace_repair_repo: None,
        plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
        events: Arc::new(NullEventSink),
        durable_recovery_state: None,
    };
    assert_eq!(
        reconcile_agent_workspace_external_pr(
            deps.clone(),
            conversation_id.clone(),
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        )
        .await
        .unwrap(),
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("workspace_missing")
    );

    workspace_repo
        .create_or_update(workspace.clone())
        .await
        .unwrap();
    assert_eq!(
        reconcile_agent_workspace_external_pr(
            deps.clone(),
            conversation_id.clone(),
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        )
        .await
        .unwrap(),
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("project_missing")
    );

    let mut archived_project = project.clone();
    archived_project.archived_at = Some(chrono::Utc::now());
    project_repo.create(archived_project.clone()).await.unwrap();
    assert_eq!(
        reconcile_agent_workspace_external_pr(
            deps.clone(),
            conversation_id.clone(),
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        )
        .await
        .unwrap(),
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("project_archived")
    );

    let mut disabled_project = project;
    disabled_project.github_pr_enabled = false;
    project_repo.update(&disabled_project).await.unwrap();
    assert_eq!(
        reconcile_agent_workspace_external_pr(
            deps,
            conversation_id,
            AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        )
        .await
        .unwrap(),
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("github_pr_disabled")
    );
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
}

#[test]
fn skip_reason_covers_non_reconcilable_workspace_shapes() {
    let project = test_project();

    let mut inactive = test_workspace_with_id(&project, "55555555-5555-5555-5555-555555555555");
    inactive.status = crate::domain::entities::AgentConversationWorkspaceStatus::Archived;
    assert_eq!(
        external_pr_reconciliation_skip_reason(&inactive),
        Some("workspace_not_active")
    );

    let mut missing_linked =
        test_workspace_with_id(&project, "55555555-5555-5555-5555-555555555556");
    missing_linked.status = AgentConversationWorkspaceStatus::Missing;
    missing_linked.publication_pr_number = Some(91);
    assert_eq!(
        external_pr_reconciliation_skip_reason(&missing_linked),
        None
    );

    let mut chat_mode = test_workspace_with_id(&project, "66666666-6666-6666-6666-666666666666");
    chat_mode.mode = AgentConversationWorkspaceMode::Chat;
    assert_eq!(
        external_pr_reconciliation_skip_reason(&chat_mode),
        Some("workspace_not_edit_mode")
    );

    let mut linked = test_workspace_with_id(&project, "77777777-7777-7777-7777-777777777777");
    linked.linked_plan_branch_id = Some(PlanBranchId::from_string("plan-branch-1"));
    assert_eq!(
        external_pr_reconciliation_skip_reason(&linked),
        Some("workspace_linked_to_plan_branch")
    );

    for (push_status, reason) in [
        ("needs_agent", "workspace_push_not_reconcilable"),
        ("pending", "workspace_push_not_reconcilable"),
        ("failed", "workspace_push_not_reconcilable"),
        ("description_failed", "workspace_push_not_reconcilable"),
    ] {
        let mut workspace = test_workspace_with_id(&project, &format!("push-status-{push_status}"));
        workspace.publication_push_status = Some(push_status.to_string());
        assert_eq!(
            external_pr_reconciliation_skip_reason(&workspace),
            Some(reason)
        );
    }

    for pr_status in ["closed", "merged"] {
        let mut workspace = test_workspace_with_id(&project, &format!("pr-status-{pr_status}"));
        workspace.publication_pr_status = Some(pr_status.to_string());
        assert_eq!(
            external_pr_reconciliation_skip_reason(&workspace),
            Some("workspace_terminal")
        );

        workspace.publication_pr_number = Some(92);
        assert_eq!(external_pr_reconciliation_skip_reason(&workspace), None);
    }
}

#[tokio::test]
async fn startup_reconciliation_processes_candidates_and_skips_blocked_projects() {
    let project = test_project();
    let blocked_project = {
        let mut project = Project::new(
            "Blocked".to_string(),
            "/tmp/ralphx-demo-blocked".to_string(),
        );
        project.base_branch = Some("main".to_string());
        project
    };
    let workspace = test_workspace_with_id(&project, "88888888-8888-8888-8888-888888888888");
    let blocked_workspace =
        test_workspace_with_id(&blocked_project, "99999999-9999-9999-9999-999999999999");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 46,
        url: "https://github.com/owner/repo/pull/46".to_string(),
        status: PrStatus::Closed,
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:20:00Z".to_string()),
        author_login: None,
    })));
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![
        project.clone(),
        blocked_project.clone(),
    ]));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo.create_or_update(workspace).await.unwrap();
    workspace_repo
        .create_or_update(blocked_workspace)
        .await
        .unwrap();
    let deps = AgentWorkspaceExternalPrReconciliationDeps {
        workspace_repo: workspace_repo.clone(),
        chat_conversation_repo: Arc::new(
            crate::infrastructure::memory::MemoryChatConversationRepository::new(),
        ),
        project_repo,
        github: github.clone(),
        clickup_integration_service: None,
        external_issue_link_service: None,
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        agent_workspace_repair_repo: None,
        plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
        events: Arc::new(NullEventSink),
        durable_recovery_state: None,
    };

    reconcile_recent_agent_workspace_external_prs_on_startup(
        deps,
        Arc::new(std::iter::once(blocked_project.id.clone()).collect()),
    )
    .await;

    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("candidate should exist");
    assert_eq!(updated.publication_pr_number, Some(46));
}

#[tokio::test]
async fn startup_reconciliation_marks_linked_failed_pr_terminal() {
    let project = test_project();
    let mut workspace = test_workspace_with_id(&project, "abababab-abab-abab-abab-abababababab");
    let conversation_id = workspace.conversation_id.clone();
    workspace.publication_pr_number = Some(264);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("failed".to_string());
    workspace.pr_supervision_status = Some("blocked".to_string());
    workspace.pr_autofix_enabled = true;
    let github = Arc::new(MockGithubService::new());
    github.will_return_status(PrStatus::Merged {
        merge_commit_sha: Some("merge-sha".to_string()),
        merged_at: None,
    });
    github.will_return_pr_detail(crate::domain::services::github_service::PrDetail {
        number: 264,
        title: "Owned PR".to_string(),
        body: None,
        author: None,
        created_at: None,
        url: None,
        state: PrStatus::Merged {
            merge_commit_sha: Some("merge-sha".to_string()),
            merged_at: None,
        },
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        base_ref_name: "main".to_string(),
    });
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let deps = AgentWorkspaceExternalPrReconciliationDeps {
        workspace_repo: workspace_repo.clone(),
        chat_conversation_repo: Arc::new(
            crate::infrastructure::memory::MemoryChatConversationRepository::new(),
        ),
        project_repo,
        github: github.clone(),
        clickup_integration_service: None,
        external_issue_link_service: None,
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        agent_workspace_repair_repo: None,
        plan_branch_repo: Arc::new(MemoryPlanBranchRepository::new()),
        events: Arc::new(NullEventSink),
        durable_recovery_state: None,
    };

    reconcile_recent_agent_workspace_external_prs_on_startup(deps, Arc::new(HashSet::new())).await;

    assert_eq!(github.state().check_pr_status_calls, 1);
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
    let updated = workspace_repo
        .get_by_conversation_id(&conversation_id)
        .await
        .unwrap()
        .expect("candidate should exist");
    assert_eq!(updated.publication_pr_status.as_deref(), Some("merged"));
    assert_eq!(updated.publication_push_status.as_deref(), Some("pushed"));
    assert_eq!(updated.pr_supervision_status, None);
}

#[tokio::test]
async fn scheduled_reconciliation_deduplicates_recent_workspace_loads_until_forced() {
    let project = test_project();
    let workspace = test_workspace_with_id(&project, "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
    let conversation_id = workspace.conversation_id.clone();
    let github = Arc::new(MockGithubService::new());
    let (deps, _workspace_repo) =
        deps_with_workspace(project, workspace.clone(), github.clone()).await;

    let factory_calls = Arc::new(AtomicUsize::new(0));
    let first_factory_calls = Arc::clone(&factory_calls);
    let first_deps = deps.clone();
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps(
        move || {
            first_factory_calls.fetch_add(1, Ordering::SeqCst);
            first_deps
        },
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );
    wait_for_latest_pr_lookup_calls(&github, 1).await;

    let duplicate_factory_calls = Arc::clone(&factory_calls);
    let duplicate_deps = deps.clone();
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps(
        move || {
            duplicate_factory_calls.fetch_add(1, Ordering::SeqCst);
            duplicate_deps
        },
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);
    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);

    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 47,
        url: "https://github.com/owner/repo/pull/47".to_string(),
        status: PrStatus::Closed,
        is_draft: false,
        head_ref_name: workspace.branch_name,
        updated_at: Some("2026-05-11T22:25:00Z".to_string()),
        author_login: None,
    })));
    let forced_factory_calls = Arc::clone(&factory_calls);
    schedule_agent_workspace_external_pr_reconciliation_with_lazy_deps(
        move || {
            forced_factory_calls.fetch_add(1, Ordering::SeqCst);
            deps
        },
        conversation_id,
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
        true,
    );
    wait_for_latest_pr_lookup_calls(&github, 2).await;
    assert_eq!(factory_calls.load(Ordering::SeqCst), 2);
}
