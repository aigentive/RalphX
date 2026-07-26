use super::*;
use crate::domain::entities::Project;

#[test]
fn pr_mode_fails_closed_for_missing_candidate_path() {
    let root = tempfile::TempDir::new().unwrap();
    let mut project = Project::new(
        "Protected".into(),
        root.path().to_string_lossy().into_owned(),
    );
    project.github_pr_enabled = true;

    assert!(protects_primary_checkout(
        &project,
        &root.path().join("missing-worktree")
    ));
}

#[test]
fn non_pr_mode_does_not_apply_primary_checkout_protection() {
    let root = tempfile::TempDir::new().unwrap();
    let mut project = Project::new("Local".into(), root.path().to_string_lossy().into_owned());
    project.github_pr_enabled = false;

    assert!(!protects_primary_checkout(&project, root.path()));
}

#[test]
fn pr_mode_protects_primary_checkout_symlink_alias() {
    let root = tempfile::TempDir::new().unwrap();
    let alias_parent = tempfile::TempDir::new().unwrap();
    let alias = alias_parent.path().join("primary-alias");
    std::os::unix::fs::symlink(root.path(), &alias).unwrap();
    let mut project = Project::new(
        "Protected".into(),
        root.path().to_string_lossy().into_owned(),
    );
    project.github_pr_enabled = true;

    assert!(protects_primary_checkout(&project, &alias));
}

#[test]
fn pr_mode_allows_existing_isolated_worktree_path() {
    let root = tempfile::TempDir::new().unwrap();
    let isolated = tempfile::TempDir::new().unwrap();
    let project = Project::new(
        "Protected".into(),
        root.path().to_string_lossy().into_owned(),
    );

    assert!(!protects_primary_checkout(&project, isolated.path()));
}
