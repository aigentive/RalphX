use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use crate::application::agent_workspace_external_pr_reconciliation::{
    external_pr_reconciliation_skip_reason, reconcile_agent_workspace_external_pr,
    reconcile_recent_agent_workspace_external_prs_on_startup,
    schedule_agent_workspace_external_pr_reconciliation,
    AgentWorkspaceExternalPrReconciliationDeps, AgentWorkspaceExternalPrReconciliationOutcome,
    AgentWorkspaceExternalPrReconciliationTrigger,
};
use crate::application::chat_service::{ChatService, MockChatService};
use crate::application::services::PrPollerRegistry;
use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentConversationWorkspaceStatus,
    ChatConversationId, IdeationAnalysisBaseRefKind, PlanBranchId, Project,
};
use crate::domain::repositories::{AgentConversationWorkspaceRepository, ProjectRepository};
use crate::domain::services::{GithubServiceTrait, PrBranchMatch, PrStatus};
use crate::infrastructure::memory::{
    MemoryAgentConversationWorkspaceRepository, MemoryAgentRunRepository,
    MemoryPlanBranchRepository, MemoryProjectRepository,
};
use crate::tests::mock_github_service::MockGithubService;

fn test_project() -> Project {
    let mut project = Project::new("Demo".to_string(), "/tmp/ralphx-demo".to_string());
    project.base_branch = Some("main".to_string());
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
            agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
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
        project_repo: project_repo.clone(),
        github: github.clone(),
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        app_handle: None,
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
        project_repo,
        github: github.clone(),
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        app_handle: None,
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
    let project_repo = Arc::new(MemoryProjectRepository::with_projects(vec![project]));
    let workspace_repo = Arc::new(MemoryAgentConversationWorkspaceRepository::new());
    workspace_repo.create_or_update(workspace).await.unwrap();
    let deps = AgentWorkspaceExternalPrReconciliationDeps {
        workspace_repo: workspace_repo.clone(),
        project_repo,
        github: github.clone(),
        pr_poller_registry: None,
        chat_service: None,
        agent_run_repo: Arc::new(MemoryAgentRunRepository::new()),
        app_handle: None,
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

    schedule_agent_workspace_external_pr_reconciliation(
        deps.clone(),
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );
    wait_for_latest_pr_lookup_calls(&github, 1).await;

    schedule_agent_workspace_external_pr_reconciliation(
        deps.clone(),
        conversation_id.clone(),
        AgentWorkspaceExternalPrReconciliationTrigger::WorkspaceLoad,
        false,
    );
    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    assert_eq!(github.state().find_latest_pr_by_head_branch_calls, 1);

    github.set_find_latest_pr_by_head_branch(Ok(Some(PrBranchMatch {
        number: 47,
        url: "https://github.com/owner/repo/pull/47".to_string(),
        status: PrStatus::Closed,
        is_draft: false,
        head_ref_name: workspace.branch_name,
        updated_at: Some("2026-05-11T22:25:00Z".to_string()),
        author_login: None,
    })));
    schedule_agent_workspace_external_pr_reconciliation(
        deps,
        conversation_id,
        AgentWorkspaceExternalPrReconciliationTrigger::AgentRunCompleted,
        true,
    );
    wait_for_latest_pr_lookup_calls(&github, 2).await;
}
