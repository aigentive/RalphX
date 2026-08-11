use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspaceSetupMode,
};
use ralphx_lib::application::agent_workspace_review::load_agent_workspace_review_context;
use ralphx_lib::application::diff_service::{DiffPageRow, DiffRefKind, DiffSide};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::diff_commands::{
    get_agent_conversation_workspace_cumulative_file_changes_for_state,
    get_agent_conversation_workspace_cumulative_file_diff_for_state,
    get_agent_conversation_workspace_file_content_range_for_state,
    get_agent_conversation_workspace_file_diff_page_for_state,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspaceBranchMode, AgentConversationWorkspaceMode,
    AgentWorkspaceReviewTargetScope, ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use std::path::Path;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[tokio::test]
async fn linked_workspace_cumulative_diff_uses_branch_merge_base() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo should be created");

    git(&repo, &["init", "-b", "master"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let branch_point = git(&repo, &["rev-parse", "HEAD"]);

    let branch_name = "feature/diverged-agent-work";
    git(&repo, &["checkout", "-b", branch_name]);
    std::fs::write(repo.join("agent-change.rs"), "pub fn agent_change() {}\n")
        .expect("agent file should be written");
    git(&repo, &["add", "agent-change.rs"]);
    git(&repo, &["commit", "-m", "agent change"]);

    git(&repo, &["checkout", "master"]);
    std::fs::write(repo.join("upstream-only.rs"), "pub fn upstream_only() {}\n")
        .expect("upstream file should be written");
    git(&repo, &["add", "upstream-only.rs"]);
    git(&repo, &["commit", "-m", "upstream change"]);
    let captured_master_tip = git(&repo, &["rev-parse", "HEAD"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Diverged Linked Workspace".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("master".to_string());
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("conversation-diverged-linked-diff");
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::LocalBranch),
            branch_mode: Some(AgentConversationWorkspaceBranchMode::Linked),
            base_ref: Some(branch_name.to_string()),
            display_name: Some(branch_name.to_string()),
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("linked workspace should prepare");
    assert_eq!(workspace.base_ref, "master");
    assert_eq!(
        workspace.base_commit.as_deref(),
        Some(captured_master_tip.as_str())
    );
    assert_ne!(
        workspace.base_commit.as_deref(),
        Some(branch_point.as_str())
    );
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");

    let changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect("cumulative changes should load");
    let paths = changes
        .into_iter()
        .map(|change| change.path)
        .collect::<Vec<_>>();

    assert_eq!(paths, vec!["agent-change.rs"]);

    let review_context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("workspace Review context should load");
    let review_target = review_context
        .target
        .expect("linked workspace should resolve a Review target");
    assert_eq!(
        review_target.scope,
        AgentWorkspaceReviewTargetScope::WorkspaceDelta
    );
    assert_eq!(review_target.base_ref, branch_point);
    assert!(review_target
        .review_packet
        .patch_excerpt
        .contains("agent-change.rs"));
    assert!(!review_target
        .review_packet
        .patch_excerpt
        .contains("upstream-only.rs"));
}

#[tokio::test]
async fn active_workspace_cumulative_diff_uses_committed_head_not_worktree() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo should be created");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Active Workspace Cumulative Diff".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    project.worktree_parent_directory =
        Some(temp.path().join("worktrees").to_string_lossy().to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("conversation-active-cumulative-diff");
    let workspace = prepare_agent_conversation_workspace_with_setup_mode(
        &project,
        &conversation_id,
        AgentConversationWorkspaceMode::Edit,
        AgentConversationWorkspaceBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            branch_mode: None,
            base_ref: Some("main".to_string()),
            display_name: None,
            source_pull_request: None,
        },
        AgentConversationWorkspaceSetupMode::Deferred,
    )
    .await
    .expect("active workspace should prepare");
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace.clone())
        .await
        .expect("workspace should persist");
    let worktree = Path::new(&workspace.worktree_path);

    std::fs::write(
        worktree.join("committed.rs"),
        "pub const VALUE: &str = \"committed\";\n",
    )
    .expect("committed file should be written");
    git(worktree, &["add", "committed.rs"]);
    git(worktree, &["commit", "-m", "committed change"]);
    std::fs::write(
        worktree.join("staged.rs"),
        "pub const STAGED: bool = true;\n",
    )
    .expect("staged file should be written");
    git(worktree, &["add", "staged.rs"]);
    std::fs::write(
        worktree.join("committed.rs"),
        "pub const VALUE: &str = \"working tree\";\n",
    )
    .expect("working tree change should be written");
    std::fs::write(
        worktree.join("untracked.rs"),
        "pub const UNTRACKED: bool = true;\n",
    )
    .expect("untracked file should be written");

    let cumulative_changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect("cumulative file changes should load");
    assert_eq!(
        cumulative_changes
            .iter()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        vec!["committed.rs"]
    );

    let cumulative_diff = get_agent_conversation_workspace_cumulative_file_diff_for_state(
        &state,
        &conversation_id,
        "committed.rs".to_string(),
    )
    .await
    .expect("cumulative file diff should load");
    assert!(cumulative_diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.content.contains("committed")));
    assert!(!cumulative_diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .any(|line| line.content.contains("working tree")));

    let cumulative_page = get_agent_conversation_workspace_file_diff_page_for_state(
        &state,
        &conversation_id,
        "committed.rs".to_string(),
        DiffRefKind::CumulativeHead,
        0,
        100,
    )
    .await
    .expect("cumulative diff page should load");
    assert!(cumulative_page.rows.iter().any(
        |row| matches!(row, DiffPageRow::Line { line } if line.content.contains("committed"))
    ));
    assert!(!cumulative_page.rows.iter().any(
        |row| matches!(row, DiffPageRow::Line { line } if line.content.contains("working tree"))
    ));

    let cumulative_range = get_agent_conversation_workspace_file_content_range_for_state(
        &state,
        &conversation_id,
        DiffSide::New,
        "committed.rs".to_string(),
        DiffRefKind::CumulativeHead,
        1,
        1,
    )
    .await
    .expect("cumulative content range should load");
    assert_eq!(
        cumulative_range[0].content,
        "pub const VALUE: &str = \"committed\";"
    );

    for (file_path, expected) in [
        ("committed.rs", "working tree"),
        ("staged.rs", "STAGED"),
        ("untracked.rs", "UNTRACKED"),
    ] {
        let head_page = get_agent_conversation_workspace_file_diff_page_for_state(
            &state,
            &conversation_id,
            file_path.to_string(),
            DiffRefKind::Head,
            0,
            100,
        )
        .await
        .expect("workspace head page should include local changes");
        assert!(head_page.rows.iter().any(
            |row| matches!(row, DiffPageRow::Line { line } if line.content.contains(expected))
        ));
    }

    let head_range = get_agent_conversation_workspace_file_content_range_for_state(
        &state,
        &conversation_id,
        DiffSide::New,
        "committed.rs".to_string(),
        DiffRefKind::Head,
        1,
        1,
    )
    .await
    .expect("workspace head content range should load");
    assert_eq!(
        head_range[0].content,
        "pub const VALUE: &str = \"working tree\";"
    );
}
