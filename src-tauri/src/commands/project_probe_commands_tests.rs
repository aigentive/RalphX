use std::path::Path;
use std::process::Command;

use super::project_probe_commands::{
    discard_project_directory, inspect_candidate_path, prepare_project_directory,
    resolve_new_project_directory, ProjectCandidate,
};
use crate::domain::entities::{Project, ProjectId};
use crate::infrastructure::git_auth::RepositoryCapability;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

fn git(path: &Path, args: &[&str]) {
    let output = Command::new(resolve_git_cli_path())
        .args(args)
        .current_dir(path)
        .output()
        .expect("git command should run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A repository with one commit on `main` and a committer identity that does not
/// depend on the developer's global git config.
fn init_repo(path: &Path) {
    git(path, &["init", "--initial-branch", "main"]);
    git(path, &["config", "user.name", "RalphX Test"]);
    git(path, &["config", "user.email", "test@localhost"]);
    std::fs::write(path.join("README.md"), "hello\n").expect("fixture file should write");
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "initial", "--no-gpg-sign"]);
}

fn project_at(directory: &Path, name: &str) -> Project {
    let mut project = Project::new(name.to_string(), directory.display().to_string());
    project.id = ProjectId::from_string(format!("project-{name}"));
    project
}

#[tokio::test]
async fn missing_path_is_not_found() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");

    let candidate = inspect_candidate_path(&directory.path().join("nope"), &[]).await;

    assert_eq!(candidate, ProjectCandidate::NotFound);
}

#[tokio::test]
async fn file_path_is_not_a_directory() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let file = directory.path().join("a-file.txt");
    std::fs::write(&file, "contents").expect("fixture file should write");

    assert_eq!(
        inspect_candidate_path(&file, &[]).await,
        ProjectCandidate::NotADirectory
    );
}

#[tokio::test]
async fn relative_path_fails_inspection_instead_of_escaping() {
    let candidate = inspect_candidate_path(Path::new("../somewhere"), &[]).await;

    assert!(
        matches!(candidate, ProjectCandidate::InspectionFailed { .. }),
        "relative candidate should fail closed, got {candidate:?}"
    );
}

#[tokio::test]
async fn empty_directory_outside_a_repository_is_empty() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let empty = directory.path().join("empty");
    std::fs::create_dir(&empty).expect("fixture directory should create");

    assert_eq!(
        inspect_candidate_path(&empty, &[]).await,
        ProjectCandidate::EmptyDirectory
    );
}

#[tokio::test]
async fn populated_non_repository_reports_entry_count() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let folder = directory.path().join("stuff");
    std::fs::create_dir(&folder).expect("fixture directory should create");
    std::fs::write(folder.join("one.txt"), "1").expect("fixture file should write");
    std::fs::write(folder.join("two.txt"), "2").expect("fixture file should write");

    assert_eq!(
        inspect_candidate_path(&folder, &[]).await,
        ProjectCandidate::NonEmptyNonRepo { entry_count: 2 }
    );
}

#[tokio::test]
async fn subdirectory_of_a_repository_reports_the_root() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());
    let nested = directory.path().join("src");
    std::fs::create_dir(&nested).expect("fixture directory should create");

    let candidate = inspect_candidate_path(&nested, &[]).await;

    let ProjectCandidate::NestedInRepository { repository_root } = candidate else {
        panic!("expected NestedInRepository, got {candidate:?}");
    };
    assert_eq!(
        Path::new(&repository_root),
        directory
            .path()
            .canonicalize()
            .expect("fixture root should canonicalize")
    );
}

#[tokio::test]
async fn repository_root_reports_branch_and_capability() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());

    let candidate = inspect_candidate_path(directory.path(), &[]).await;

    let ProjectCandidate::Repository {
        current_branch,
        default_branch,
        branches,
        has_commits,
        is_dirty,
        capability,
        already_registered_as,
        ..
    } = candidate
    else {
        panic!("expected Repository verdict");
    };
    assert_eq!(current_branch, "main");
    assert_eq!(default_branch.as_deref(), Some("main"));
    assert!(branches.contains(&"main".to_string()));
    assert!(has_commits);
    assert!(!is_dirty);
    assert_eq!(capability, RepositoryCapability::LocalOnly);
    assert_eq!(already_registered_as, None);
}

#[tokio::test]
async fn uncommitted_changes_are_reported_as_dirty() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());
    std::fs::write(directory.path().join("README.md"), "changed\n")
        .expect("fixture file should write");

    let candidate = inspect_candidate_path(directory.path(), &[]).await;

    let ProjectCandidate::Repository { is_dirty, .. } = candidate else {
        panic!("expected Repository verdict");
    };
    assert!(is_dirty);
}

#[tokio::test]
async fn detached_head_is_its_own_verdict() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());
    git(directory.path(), &["checkout", "--detach", "HEAD"]);

    assert!(
        matches!(
            inspect_candidate_path(directory.path(), &[]).await,
            ProjectCandidate::DetachedHead { .. }
        ),
        "detached HEAD should not be reported as a usable repository"
    );
}

#[tokio::test]
async fn unborn_repository_reports_no_commits() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    git(directory.path(), &["init", "--initial-branch", "main"]);

    let candidate = inspect_candidate_path(directory.path(), &[]).await;

    let ProjectCandidate::Repository {
        has_commits,
        current_branch,
        ..
    } = candidate
    else {
        panic!("expected Repository verdict for an unborn repository");
    };
    assert!(!has_commits);
    assert_eq!(current_branch, "main");
}

#[tokio::test]
async fn duplicate_detection_survives_case_differences_and_trailing_slashes() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let repo = directory.path().join("MyRepo");
    std::fs::create_dir(&repo).expect("fixture directory should create");
    init_repo(&repo);

    // Registered with a trailing separator, which is how a folder picker can
    // hand a path back; canonicalization is what makes the match hold.
    let registered = project_at(Path::new(&format!("{}/", repo.display())), "existing");
    let candidate = inspect_candidate_path(&repo, std::slice::from_ref(&registered)).await;

    let ProjectCandidate::Repository {
        already_registered_as,
        ..
    } = candidate
    else {
        panic!("expected Repository verdict");
    };
    let duplicate = already_registered_as.expect("registered project should be detected");
    assert_eq!(duplicate.id, "project-existing");
    assert_eq!(duplicate.name, "existing");
}

#[tokio::test]
async fn probing_never_writes_to_the_candidate() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let folder = directory.path().join("untouched");
    std::fs::create_dir(&folder).expect("fixture directory should create");

    inspect_candidate_path(&folder, &[]).await;

    let entries: Vec<_> = std::fs::read_dir(&folder)
        .expect("fixture directory should read")
        .collect();
    assert!(entries.is_empty(), "probe must not create anything");
}

// ── prepare_new_project_directory ────────────────────────────────────────────

#[test]
fn folder_names_that_could_escape_the_parent_are_rejected() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let parent = directory.path().display().to_string();

    for name in ["..", ".", "", "   ", "~", "~/elsewhere", "a/b", "a\\b"] {
        assert!(
            resolve_new_project_directory(&parent, name).is_err(),
            "folder name {name:?} should be rejected"
        );
    }
}

#[test]
fn relative_parent_directory_is_rejected() {
    assert!(resolve_new_project_directory("relative/parent", "app").is_err());
}

#[test]
fn valid_name_joins_under_the_canonical_parent() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let parent = directory.path().display().to_string();

    let resolved =
        resolve_new_project_directory(&parent, " my-app ").expect("valid name should resolve");

    assert_eq!(
        resolved,
        directory
            .path()
            .canonicalize()
            .expect("parent should canonicalize")
            .join("my-app")
    );
}

#[tokio::test]
async fn preparing_a_missing_directory_creates_it() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");

    let prepared = prepare_project_directory(&directory.path().display().to_string(), "fresh")
        .await
        .expect("preparation should succeed");

    assert!(prepared.created);
    assert!(Path::new(&prepared.path).is_dir());
}

#[tokio::test]
async fn preparing_an_existing_empty_directory_does_not_claim_creation() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    std::fs::create_dir(directory.path().join("already")).expect("fixture directory should create");

    let prepared = prepare_project_directory(&directory.path().display().to_string(), "already")
        .await
        .expect("preparation should accept an empty directory");

    assert!(!prepared.created);
}

#[tokio::test]
async fn preparing_a_populated_directory_is_refused() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let occupied = directory.path().join("occupied");
    std::fs::create_dir(&occupied).expect("fixture directory should create");
    std::fs::write(occupied.join("keep.txt"), "user data").expect("fixture file should write");

    assert!(
        prepare_project_directory(&directory.path().display().to_string(), "occupied")
            .await
            .is_err(),
        "a directory with contents must not be silently reused"
    );
}

// ── rollback ─────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rollback_removes_a_directory_holding_only_bootstrap_git_metadata() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let prepared = prepare_project_directory(&directory.path().display().to_string(), "rollback")
        .await
        .expect("preparation should succeed");
    git(Path::new(&prepared.path), &["init"]);

    discard_project_directory(Path::new(&prepared.path))
        .await
        .expect("rollback should remove the prepared directory");

    assert!(!Path::new(&prepared.path).exists());
}

#[tokio::test]
async fn rollback_refuses_to_remove_user_content() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let occupied = directory.path().join("mine");
    std::fs::create_dir(&occupied).expect("fixture directory should create");
    std::fs::write(occupied.join("important.txt"), "user data").expect("fixture file should write");

    assert!(
        discard_project_directory(&occupied).await.is_err(),
        "rollback must never delete content RalphX did not create"
    );
    assert!(occupied.join("important.txt").exists());
}

#[tokio::test]
async fn rollback_of_a_missing_directory_is_a_no_op() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");

    discard_project_directory(&directory.path().join("gone"))
        .await
        .expect("rollback should tolerate an already-removed directory");
}

// ── worktree parent verdicts (proof obligation 14) ───────────────────────────

use super::project_probe_commands::{worktree_parent_verdict, WorktreeParentVerdict};

#[test]
fn a_normal_parent_folder_is_accepted() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");

    let verdict = worktree_parent_verdict(&directory.path().display().to_string(), None);

    assert!(
        matches!(verdict, WorktreeParentVerdict::Ok { .. }),
        "expected Ok, got {verdict:?}"
    );
}

/// The tilde form is what the default worktree parent actually looks like, so
/// the dialog must expand it the same way execution later will.
#[test]
fn a_tilde_path_is_expanded_before_it_is_judged() {
    let verdict = worktree_parent_verdict("~/ralphx-worktrees-does-not-exist", None);

    let WorktreeParentVerdict::NotFound { path } = verdict else {
        panic!("expected NotFound for an unexpanded-looking path, got {verdict:?}");
    };
    assert!(
        !path.starts_with('~'),
        "the verdict should report the expanded path, got {path}"
    );
    assert!(path.ends_with("ralphx-worktrees-does-not-exist"));
}

#[test]
fn a_missing_parent_is_reported_as_not_found() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");

    let verdict = worktree_parent_verdict(
        &directory.path().join("nowhere").display().to_string(),
        None,
    );

    assert!(matches!(verdict, WorktreeParentVerdict::NotFound { .. }));
}

#[test]
fn a_file_is_reported_as_not_a_directory() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let file = directory.path().join("a-file");
    std::fs::write(&file, "contents").expect("fixture file should write");

    let verdict = worktree_parent_verdict(&file.display().to_string(), None);

    assert!(matches!(
        verdict,
        WorktreeParentVerdict::NotADirectory { .. }
    ));
}

/// A worktree parent inside the repository would make every task worktree show
/// up as untracked changes in the project itself.
#[test]
fn a_parent_inside_the_repository_fails_closed() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());
    let inside = directory.path().join("worktrees");
    std::fs::create_dir(&inside).expect("fixture directory should create");

    let verdict = worktree_parent_verdict(
        &inside.display().to_string(),
        Some(&directory.path().display().to_string()),
    );

    assert!(
        matches!(verdict, WorktreeParentVerdict::InsideRepository { .. }),
        "expected InsideRepository, got {verdict:?}"
    );
}

#[test]
fn the_repository_root_itself_is_inside_the_repository() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    init_repo(directory.path());
    let root = directory.path().display().to_string();

    assert!(matches!(
        worktree_parent_verdict(&root, Some(&root)),
        WorktreeParentVerdict::InsideRepository { .. }
    ));
}

#[test]
fn a_sibling_of_the_repository_is_accepted() {
    let directory = tempfile::tempdir().expect("temporary directory should exist");
    let repo = directory.path().join("repo");
    std::fs::create_dir(&repo).expect("fixture directory should create");
    init_repo(&repo);
    let sibling = directory.path().join("worktrees");
    std::fs::create_dir(&sibling).expect("fixture directory should create");

    let verdict = worktree_parent_verdict(
        &sibling.display().to_string(),
        Some(&repo.display().to_string()),
    );

    assert!(
        matches!(verdict, WorktreeParentVerdict::Ok { .. }),
        "expected Ok, got {verdict:?}"
    );
}

#[test]
fn traversal_and_relative_parents_are_rejected() {
    for path in ["../elsewhere", "relative/path", "/tmp/../etc"] {
        assert!(
            matches!(
                worktree_parent_verdict(path, None),
                WorktreeParentVerdict::Invalid { .. }
            ),
            "{path} should be rejected"
        );
    }
}
