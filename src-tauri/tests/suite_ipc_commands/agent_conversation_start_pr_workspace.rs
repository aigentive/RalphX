use super::agent_conversation_start_support::*;
use ralphx_lib::domain::execution::ExecutionSettings;

#[tokio::test]
async fn ipc_contract_start_service_pr_backed_local_branch_prepares_isolated_workspace() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-source-pr";
    git(&repo_path, &["checkout", "-b", branch]);
    std::fs::write(repo_path.join("README.md"), "source pr\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "source pr"]);
    let head_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-success",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let result = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Start from PR",
            "edit",
            Some(branch),
            None,
            None,
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 42,
                url: Some("https://github.com/owner/repo/pull/42".to_string()),
                title: Some("Service source PR".to_string()),
                head_ref_name: branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(head_sha.clone()),
            }),
        ),
    )
    .await
    .expect("service start should queue while execution is paused");

    assert!(result.send_result.was_queued);
    let workspace = result.workspace.expect("edit mode creates workspace");
    assert_eq!(
        workspace.branch_mode,
        AgentConversationWorkspaceBranchMode::Isolated
    );
    assert_eq!(
        workspace.base_ref_kind,
        IdeationAnalysisBaseRefKind::LocalBranch
    );
    assert_eq!(workspace.base_ref, branch);
    assert_ne!(workspace.branch_name, branch);
    assert_eq!(workspace.publication_pr_number, None);
    assert_eq!(
        workspace
            .source_pull_request
            .as_ref()
            .map(|source| source.number),
        Some(42)
    );
}

#[tokio::test]
async fn ipc_contract_start_service_review_pr_creates_enabled_monitor() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-review-pr-monitor";
    git(&repo_path, &["checkout", "-b", branch]);
    std::fs::write(repo_path.join("README.md"), "review pr\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "review pr"]);
    let head_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-review-pr-monitor",
        &repo_path,
        &worktree_parent,
    )
    .await;
    state
        .execution_settings_repo
        .update_settings(
            Some(&project.id),
            &ExecutionSettings {
                agent_workspace_pr_autofix_default: true,
                agent_workspace_pr_auto_merge_default: true,
                ..ExecutionSettings::default()
            },
        )
        .await
        .expect("project automation defaults should persist");
    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let result = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Review this PR",
            "review_pr",
            Some(branch),
            None,
            None,
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 77,
                url: Some("https://github.com/owner/repo/pull/77".to_string()),
                title: Some("Review PR monitor".to_string()),
                head_ref_name: branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(head_sha.clone()),
            }),
        ),
    )
    .await
    .expect("Review PR start should queue while execution is paused");

    assert!(result.send_result.was_queued);
    let workspace = result.workspace.expect("Review PR mode creates workspace");
    assert_eq!(workspace.mode, AgentConversationWorkspaceMode::ReviewPr);
    assert!(!workspace.pr_autofix_enabled);
    assert!(!workspace.pr_auto_merge_desired);
    let monitor = app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("Review PR start should arm monitor");
    assert_eq!(monitor.pr_number, 77);
    assert_eq!(
        monitor.last_seen_head_sha.as_deref(),
        Some(head_sha.as_str())
    );
    assert!(monitor.monitor_enabled);
    assert_eq!(
        monitor.status,
        AgentWorkspacePrReviewMonitorStatus::Watching
    );
}

#[tokio::test]
async fn ipc_contract_start_service_review_pr_preserves_existing_monitor() {
    ralphx_lib::testing::seed_available_harness_probes_for_test();
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo_path = temp.path().join("repo");
    let worktree_parent = temp.path().join("worktrees");
    setup_repo(&repo_path);
    let branch = "feature/service-review-pr-existing-monitor";
    git(&repo_path, &["checkout", "-b", branch]);
    std::fs::write(repo_path.join("README.md"), "review pr existing\n")
        .expect("fixture update should be written");
    git(&repo_path, &["add", "README.md"]);
    git(&repo_path, &["commit", "-m", "review pr existing"]);
    let head_sha = git(&repo_path, &["rev-parse", "HEAD"]);
    git(&repo_path, &["checkout", "main"]);

    let state = AppState::new_test();
    let project = seed_project(
        &state,
        "project-start-service-review-pr-existing-monitor",
        &repo_path,
        &worktree_parent,
    )
    .await;
    let conversation = state
        .chat_conversation_repo
        .create(ChatConversation::new_project(project.id.clone()))
        .await
        .expect("conversation should persist");
    let mut existing_monitor = AgentWorkspacePrReviewMonitor::new(
        conversation.id,
        project.id.clone(),
        88,
        Some("previous-head".to_string()),
    );
    existing_monitor.monitor_enabled = false;
    existing_monitor.status = AgentWorkspacePrReviewMonitorStatus::Paused;
    existing_monitor.first_review_completed = true;
    state
        .agent_conversation_workspace_repo
        .upsert_pr_review_monitor(existing_monitor)
        .await
        .expect("existing monitor should persist");

    let execution_state = Arc::new(ExecutionState::new());
    execution_state.pause();
    let app = build_app(state, execution_state);

    let result = start_with_app(
        &app,
        service_start_input(
            &project.id,
            "Review this PR without replacing existing monitor state",
            "review_pr",
            Some(branch),
            None,
            Some(&conversation.id),
            Some(AgentWorkspaceSourcePullRequestInput {
                number: 88,
                url: Some("https://github.com/owner/repo/pull/88".to_string()),
                title: Some("Review PR existing monitor".to_string()),
                head_ref_name: branch.to_string(),
                base_ref_name: Some("main".to_string()),
                head_ref_oid: Some(head_sha),
            }),
        ),
    )
    .await
    .expect("Review PR start should queue while execution is paused");

    assert!(result.send_result.was_queued);
    let workspace = result.workspace.expect("Review PR mode creates workspace");
    let monitor = app
        .state::<AppState>()
        .agent_conversation_workspace_repo
        .get_pr_review_monitor(&workspace.conversation_id)
        .await
        .expect("monitor lookup should succeed")
        .expect("existing monitor should remain present");
    assert_eq!(monitor.pr_number, 88);
    assert_eq!(
        monitor.last_seen_head_sha.as_deref(),
        Some("previous-head"),
        "start should not replace already-managed monitor state"
    );
    assert!(!monitor.monitor_enabled);
    assert_eq!(monitor.status, AgentWorkspacePrReviewMonitorStatus::Paused);
    assert!(monitor.first_review_completed);
}
