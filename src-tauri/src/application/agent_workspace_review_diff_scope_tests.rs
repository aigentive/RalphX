use std::collections::BTreeSet;

use super::agent_workspace_review::resolve_review_target;
use super::agent_workspace_review_diff::{
    ensure_workspace_review_snapshot_current, full_hunk_anchors_for_requests,
    get_workspace_review_diff_page, list_workspace_review_files, AgentWorkspaceReviewDiffSource,
};
use super::agent_workspace_review_diff_cursor::{decode_cursor, ReviewDiffCursorKind};
use super::agent_workspace_review_diff_tests::{git, init_workspace};
use crate::domain::entities::IdeationAnalysisBaseRefKind;
use crate::error::AppError;

#[tokio::test]
async fn selected_source_scope_inventory_and_diff_page_use_the_selected_branch() {
    let (repo, mut workspace, project) = init_workspace();
    git(repo.path(), &["checkout", "-b", "feature/source"]);
    std::fs::write(repo.path().join("selected.rs"), "pub fn selected() {}\n")
        .expect("write selected source file");
    git(repo.path(), &["add", "selected.rs"]);
    git(repo.path(), &["commit", "-m", "selected source"]);
    git(repo.path(), &["checkout", "main"]);
    workspace.base_ref_kind = IdeationAnalysisBaseRefKind::LocalBranch;
    workspace.base_ref = "feature/source".to_string();
    workspace.base_commit = None;
    workspace.worktree_path = repo
        .path()
        .join("missing-worktree")
        .to_string_lossy()
        .to_string();

    let inventory = list_workspace_review_files(&workspace, &project, None, None)
        .await
        .expect("selected source inventory");
    assert_eq!(inventory.total_count, 1);
    assert_eq!(inventory.files[0].path, "selected.rs");
    assert_eq!(inventory.files[0].sources, vec!["selected_source"]);

    let diff = get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("selected.rs"),
        Some("selected_source"),
        None,
    )
    .await
    .expect("selected source diff");
    assert_eq!(diff.source, AgentWorkspaceReviewDiffSource::SelectedSource);
    assert_eq!(diff.page.file_path, "selected.rs");
    assert!(!diff.hunk_anchors.is_empty());
}

#[tokio::test]
async fn full_hunk_requests_filter_invalid_selections_and_reject_stale_snapshots() {
    let (repo, workspace, project) = init_workspace();
    std::fs::write(repo.path().join("README.md"), "changed\n").expect("modify tracked file");
    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("resolve review target")
        .expect("current workspace target");
    let selections = BTreeSet::from([
        ("README.md".to_string(), "unstaged".to_string()),
        ("README.md".to_string(), "unknown".to_string()),
        ("README.md".to_string(), "selected_source".to_string()),
        (" ".to_string(), "unstaged".to_string()),
    ]);

    let (anchors, source_fingerprint) =
        full_hunk_anchors_for_requests(&workspace, &project, &target.diff_fingerprint, &selections)
            .await
            .expect("valid selections should resolve exact hunks");
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].path, "README.md");
    assert_eq!(anchors[0].source, "unstaged");
    ensure_workspace_review_snapshot_current(
        &workspace,
        &project,
        &target.diff_fingerprint,
        &source_fingerprint,
    )
    .await
    .expect("current snapshot should remain valid");

    let stale = full_hunk_anchors_for_requests(&workspace, &project, "stale", &selections)
        .await
        .expect_err("stale target fingerprint must fail closed");
    assert!(matches!(stale, AppError::Conflict(_)));
    let stale = ensure_workspace_review_snapshot_current(
        &workspace,
        &project,
        &target.diff_fingerprint,
        "stale",
    )
    .await
    .expect_err("stale source fingerprint must fail closed");
    assert!(matches!(stale, AppError::Conflict(_)));
}

#[test]
fn cursor_text_bounds_reject_empty_and_oversized_values() {
    assert!(decode_cursor("", ReviewDiffCursorKind::Files).is_err());
    assert!(decode_cursor(&"x".repeat(8_193), ReviewDiffCursorKind::Files).is_err());
}
