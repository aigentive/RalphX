use ralphx_lib::application::agent_workspace_review::load_agent_workspace_review_context;
use ralphx_lib::application::AppState;
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceReviewTargetScope,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project,
};
use ralphx_lib::error::AppError;
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
async fn review_pr_workspace_rejects_workspace_review_context() {
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::from_string("review-pr-workspace-review".to_string()),
        ralphx_lib::domain::entities::ProjectId::from_string("project-review-pr".to_string()),
        AgentConversationWorkspaceMode::ReviewPr,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("main".to_string()),
        None,
        "ralphx/review-pr".to_string(),
        "/tmp/ralphx-review-pr".to_string(),
    );

    let error = load_agent_workspace_review_context(&AppState::new_test(), &workspace)
        .await
        .expect_err("Review PR workspaces must not expose Workspace Review");

    assert!(
        matches!(error, AppError::Validation(message) if message.contains("unavailable in Review PR mode"))
    );
}

#[tokio::test]
async fn merged_published_workspace_review_context_uses_preserved_pr_head() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo dir should be created");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let initial_main = git(&repo, &["rev-parse", "HEAD"]);

    git(
        &repo,
        &["checkout", "-b", "ralphx/demo/agent-conversation-1"],
    );
    std::fs::write(repo.join("src.rs"), "pub fn answer() -> i32 { 42 }\n")
        .expect("feature file should be written");
    git(&repo, &["add", "src.rs"]);
    git(&repo, &["commit", "-m", "feat: add answer"]);
    let pr_head = git(&repo, &["rev-parse", "HEAD"]);
    let pr_head_ref = "refs/ralphx/pr-heads/351";
    git(&repo, &["update-ref", pr_head_ref, &pr_head]);

    git(&repo, &["checkout", "main"]);
    git(&repo, &["merge", "--squash", pr_head_ref]);
    git(&repo, &["commit", "-m", "Squash PR 351"]);
    let squash_commit = git(&repo, &["rev-parse", "HEAD"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Merged Workspace Review".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("conversation-merged-review");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(squash_commit),
        "ralphx/demo/agent-conversation-1".to_string(),
        temp.path()
            .join("removed-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.publication_pr_number = Some(351);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());

    let context = load_agent_workspace_review_context(&state, &workspace)
        .await
        .expect("merged workspace Review context should load");

    assert!(context.should_show_tab);
    let target = context
        .target
        .expect("merged published workspace should resolve a Review target");
    assert_eq!(
        target.scope,
        AgentWorkspaceReviewTargetScope::SelectedSource
    );
    assert_eq!(target.base_ref, initial_main);
    assert_eq!(target.head_ref, pr_head_ref);
    assert_eq!(target.head_sha.as_deref(), Some(pr_head.as_str()));
    assert_eq!(target.source_pull_request_number, Some(351));
    assert_eq!(
        context.monitor.selected_source_pull_request_number,
        Some(351)
    );
}
