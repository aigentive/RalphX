use super::ideation_workspace::{prepare_ideation_analysis_state, IdeationAnalysisBaseSelection};
use crate::domain::entities::{
    IdeationAnalysisBaseRefKind, IdeationAnalysisWorkspaceKind, IdeationSessionId, Project,
};
use crate::error::AppError;
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

fn setup_repo() -> (tempfile::TempDir, Project, String) {
    let temp = tempfile::tempdir().expect("tempdir should be created");
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).expect("repo directory should be created");

    let init = Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(&repo)
        .output()
        .expect("repo should initialize");
    assert!(
        init.status.success(),
        "git init failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init.stdout),
        String::from_utf8_lossy(&init.stderr)
    );
    git(&repo, &["config", "user.email", "test@example.com"]);
    git(&repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join("README.md"), "base\n").expect("base file should be written");
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-m", "base"]);
    let main_sha = git(&repo, &["rev-parse", "HEAD"]);
    git(&repo, &["checkout", "-b", "feature/current"]);

    let mut project = Project::new(
        "Ideation Workspace".to_string(),
        repo.to_string_lossy().to_string(),
    );
    project.base_branch = Some("main".to_string());
    (temp, project, main_sha)
}

#[tokio::test]
async fn project_default_selection_uses_resolved_default_when_current_branch_differs() {
    let (_temp, project, main_sha) = setup_repo();
    let session_id = IdeationSessionId::from_string("ideation-workspace-main");

    let analysis = prepare_ideation_analysis_state(
        &project,
        &session_id,
        IdeationAnalysisBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            base_ref: Some("main".to_string()),
            display_name: None,
        },
    )
    .await
    .expect("project default selection should resolve");

    assert_eq!(
        analysis.base_ref_kind,
        Some(IdeationAnalysisBaseRefKind::ProjectDefault)
    );
    assert_eq!(analysis.base_ref.as_deref(), Some("main"));
    assert_eq!(analysis.base_commit.as_deref(), Some(main_sha.as_str()));
    assert_eq!(
        analysis.workspace_kind,
        IdeationAnalysisWorkspaceKind::IdeationWorktree
    );
    assert_ne!(
        analysis.workspace_path.as_deref(),
        Some(project.working_directory.as_str())
    );
}

#[tokio::test]
async fn project_default_selection_rejects_mismatched_selected_ref() {
    let (_temp, project, _main_sha) = setup_repo();
    let session_id = IdeationSessionId::from_string("ideation-workspace-mismatch");

    let err = prepare_ideation_analysis_state(
        &project,
        &session_id,
        IdeationAnalysisBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            base_ref: Some("feature/current".to_string()),
            display_name: None,
        },
    )
    .await
    .expect_err("mislabeled project default ref should be rejected");

    assert!(
        matches!(err, AppError::Validation(message) if message.contains(
            "Project default ideation base ref 'feature/current' does not match resolved project default 'main'"
        ))
    );
}

#[tokio::test]
async fn project_default_selection_ignores_blank_selected_ref() {
    let (_temp, project, main_sha) = setup_repo();
    let session_id = IdeationSessionId::from_string("ideation-workspace-blank");

    let analysis = prepare_ideation_analysis_state(
        &project,
        &session_id,
        IdeationAnalysisBaseSelection {
            kind: Some(IdeationAnalysisBaseRefKind::ProjectDefault),
            base_ref: Some("   ".to_string()),
            display_name: None,
        },
    )
    .await
    .expect("blank project default ref should fall back to resolved default");

    assert_eq!(analysis.base_ref.as_deref(), Some("main"));
    assert_eq!(analysis.base_commit.as_deref(), Some(main_sha.as_str()));
}
