use std::path::PathBuf;
use std::sync::Arc;

use crate::application::agent_workspace_external_pr_reconciliation::{
    reconcile_agent_workspace_external_pr, AgentWorkspaceExternalPrReconciliationDeps,
    AgentWorkspaceExternalPrReconciliationOutcome, AgentWorkspaceExternalPrReconciliationTrigger,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
};
use crate::domain::repositories::AgentConversationWorkspaceRepository;
use crate::domain::services::{PrBranchMatch, PrStatus};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn test_project() -> Project {
    let mut project = Project::new("Demo".to_string(), "/tmp/ralphx-demo".to_string());
    project.base_branch = Some("main".to_string());
    project
}

fn test_workspace(project: &Project) -> AgentConversationWorkspace {
    let conversation_id = ChatConversationId::from_string("11111111-1111-1111-1111-111111111111");
    AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some("base-sha".to_string()),
        "ralphx/demo/agent-11111111".to_string(),
        PathBuf::from("/tmp/ralphx-demo-worktree")
            .to_string_lossy()
            .to_string(),
    )
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
        .create_or_update(workspace)
        .await
        .expect("workspace should save");

    (
        AgentWorkspaceExternalPrReconciliationDeps {
            workspace_repo: workspace_repo.clone(),
            project_repo,
            github,
            pr_poller_registry: None,
            chat_service: None,
            app_handle: None,
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
    })));
    let (deps, workspace_repo) =
        deps_with_workspace(project, workspace.clone(), github.clone()).await;

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
        },
        is_draft: false,
        head_ref_name: workspace.branch_name.clone(),
        updated_at: Some("2026-05-11T22:05:00Z".to_string()),
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
async fn reconciliation_skips_workspace_that_already_has_pr_number() {
    let project = test_project();
    let mut workspace = test_workspace(&project);
    let conversation_id = workspace.conversation_id.clone();
    workspace.publication_pr_number = Some(99);
    workspace.publication_pr_status = Some("open".to_string());
    let github = Arc::new(MockGithubService::new());
    let (deps, _workspace_repo) = deps_with_workspace(project, workspace, github.clone()).await;

    let outcome = reconcile_agent_workspace_external_pr(
        deps,
        conversation_id,
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(
        outcome,
        AgentWorkspaceExternalPrReconciliationOutcome::Skipped("workspace_already_linked")
    );
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 0);
}
