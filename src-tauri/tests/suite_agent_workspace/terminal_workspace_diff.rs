use ralphx_lib::application::agent_conversation_workspace::resolve_agent_conversation_workspace_path;
use ralphx_lib::application::diff_service::{DiffLineKind, DiffPageRow, DiffRefKind};
use ralphx_lib::application::AppState;
use ralphx_lib::commands::diff_commands::{
    get_agent_conversation_workspace_cumulative_file_changes_for_state,
    get_agent_conversation_workspace_file_diff_page_for_state,
};
use ralphx_lib::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, ChatConversationId,
    IdeationAnalysisBaseRefKind, Project,
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

fn git_ref_exists(repo: &Path, reference: &str) -> bool {
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", reference])
        .current_dir(repo)
        .status()
        .expect("git show-ref should spawn")
        .success()
}

#[tokio::test]
async fn terminal_workspace_diff_pages_use_preserved_pr_head_without_restoring_branch() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo should be created");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);

    let workspace_branch = "ralphx/demo/terminal-diff";
    git(&repo, &["checkout", "-b", workspace_branch]);
    std::fs::write(
        repo.join("published.rs"),
        "pub fn published_answer() -> i32 { 42 }\n",
    )
    .expect("published file should be written");
    git(&repo, &["add", "published.rs"]);
    git(&repo, &["commit", "-m", "feat: publish answer"]);
    let pr_head = git(&repo, &["rev-parse", "HEAD"]);
    let pr_head_ref = "refs/ralphx/pr-heads/451";
    git(&repo, &["update-ref", pr_head_ref, &pr_head]);

    git(&repo, &["checkout", "main"]);
    std::fs::write(repo.join("upstream-only.rs"), "pub fn upstream_only() {}\n")
        .expect("upstream file should be written");
    git(&repo, &["add", "upstream-only.rs"]);
    git(&repo, &["commit", "-m", "chore: upstream-only change"]);
    git(&repo, &["merge", "--squash", pr_head_ref]);
    git(&repo, &["commit", "-m", "Squash PR 451"]);
    let merged_base_commit = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["branch", "-D", workspace_branch]);
    assert!(!git_ref_exists(
        &repo,
        &format!("refs/heads/{workspace_branch}")
    ));

    let state = AppState::new_test();
    let mut project = Project::new(
        "Terminal Workspace Diff".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("conversation-terminal-diff");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(merged_base_commit),
        workspace_branch.to_string(),
        temp.path()
            .join("removed-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.publication_pr_number = Some(451);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect("terminal cumulative files should load from the preserved PR head");
    assert_eq!(
        changes
            .iter()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        vec!["published.rs"]
    );

    let page = get_agent_conversation_workspace_file_diff_page_for_state(
        &state,
        &conversation_id,
        "published.rs".to_string(),
        DiffRefKind::CumulativeHead,
        0,
        100,
    )
    .await
    .expect("terminal cumulative diff page should load from the preserved PR head");
    assert_eq!(page.file_path, "published.rs");
    assert!(page.rows.iter().any(|row| {
        matches!(
            row,
            DiffPageRow::Line { line }
                if line.kind == DiffLineKind::Addition
                    && line.content.contains("published_answer")
        )
    }));
    assert!(!page.rows.iter().any(|row| {
        matches!(
            row,
            DiffPageRow::Line { line } if line.content.contains("upstream_only")
        )
    }));
    assert!(!git_ref_exists(
        &repo,
        &format!("refs/heads/{workspace_branch}")
    ));
}

#[tokio::test]
async fn terminal_transition_does_not_reuse_cached_active_branch_context() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo should be created");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_commit = git(&repo, &["rev-parse", "HEAD"]);

    let workspace_branch = "ralphx/demo/terminal-cache";
    git(&repo, &["checkout", "-b", workspace_branch]);
    std::fs::write(repo.join("published.rs"), "pub fn published() {}\n")
        .expect("published file should be written");
    git(&repo, &["add", "published.rs"]);
    git(&repo, &["commit", "-m", "feat: published change"]);
    let pr_head = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["update-ref", "refs/ralphx/pr-heads/452", &pr_head]);
    std::fs::write(repo.join("active-only.rs"), "pub fn active_only() {}\n")
        .expect("active-only file should be written");
    git(&repo, &["add", "active-only.rs"]);
    git(&repo, &["commit", "-m", "feat: unpublished active change"]);
    git(&repo, &["checkout", "main"]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Terminal Workspace Cache".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("00000000-0000-4000-8000-000000000452");
    let expected_worktree = resolve_agent_conversation_workspace_path(&project, &conversation_id)
        .expect("canonical worktree path should resolve");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_commit),
        workspace_branch.to_string(),
        expected_worktree.to_string_lossy().to_string(),
    );
    workspace.publication_pr_number = Some(452);
    workspace.publication_pr_status = Some("open".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("workspace should persist");

    let active_changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect("active cumulative files should load from the workspace branch");
    assert!(active_changes
        .iter()
        .any(|change| change.path == "active-only.rs"));

    state
        .agent_conversation_workspace_repo
        .update_publication(
            &conversation_id,
            Some(452),
            Some("https://github.com/mock/project/pull/452"),
            Some("merged"),
            Some("pushed"),
        )
        .await
        .expect("workspace should transition to merged");
    git(&repo, &["branch", "-D", workspace_branch]);

    let terminal_changes = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect("terminal cumulative files should refresh to the preserved PR head");
    assert_eq!(
        terminal_changes
            .iter()
            .map(|change| change.path.as_str())
            .collect::<Vec<_>>(),
        vec!["published.rs"]
    );
}

#[tokio::test]
async fn terminal_workspace_with_missing_preserved_pr_head_fails_closed() {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).expect("repo should be created");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let base_commit = git(&repo, &["rev-parse", "HEAD"]);

    let workspace_branch = "ralphx/demo/missing-terminal-head";
    git(&repo, &["checkout", "-b", workspace_branch]);
    std::fs::write(repo.join("missing.rs"), "pub fn missing_head() {}\n")
        .expect("workspace file should be written");
    git(&repo, &["add", "missing.rs"]);
    git(&repo, &["commit", "-m", "feat: missing terminal head"]);
    git(&repo, &["checkout", "main"]);
    git(&repo, &["branch", "-D", workspace_branch]);

    let state = AppState::new_test();
    let mut project = Project::new(
        "Missing Terminal Head".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    state
        .project_repo
        .create(project.clone())
        .await
        .expect("project should persist");

    let conversation_id = ChatConversationId::from_string("conversation-missing-terminal-head");
    let mut workspace = AgentConversationWorkspace::new(
        conversation_id,
        project.id.clone(),
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_commit),
        workspace_branch.to_string(),
        temp.path()
            .join("removed-worktree")
            .to_string_lossy()
            .to_string(),
    );
    workspace.publication_pr_number = Some(453);
    workspace.publication_pr_status = Some("merged".to_string());
    workspace.publication_push_status = Some("pushed".to_string());
    state
        .agent_conversation_workspace_repo
        .create_or_update(workspace)
        .await
        .expect("terminal workspace should persist");

    let error = get_agent_conversation_workspace_cumulative_file_changes_for_state(
        &state,
        &conversation_id,
    )
    .await
    .expect_err("missing preserved PR head must not fall back to another range");
    assert!(error.to_string().contains("refs/ralphx/pr-heads/453"));
    assert!(!git_ref_exists(
        &repo,
        &format!("refs/heads/{workspace_branch}")
    ));
}
