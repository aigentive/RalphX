use ralphx_lib::application::agent_conversation_workspace::{
    prepare_agent_conversation_workspace_with_setup_mode, AgentConversationWorkspaceBaseSelection,
    AgentConversationWorkspaceSetupMode,
};
use ralphx_lib::application::agent_workspace_review::load_agent_workspace_review_context;
use ralphx_lib::application::AppState;
use ralphx_lib::commands::diff_commands::get_agent_conversation_workspace_cumulative_file_changes_for_state;
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
