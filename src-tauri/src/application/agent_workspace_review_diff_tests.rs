use super::agent_workspace_review::resolve_review_target;
use super::agent_workspace_review_diff::*;
use super::agent_workspace_review_diff_cursor::*;

use std::path::Path;
use std::process::Command;

use crate::domain::entities::{
    AgentConversationWorkspace, AgentConversationWorkspaceMode, AgentWorkspaceReviewTargetScope,
    ChatConversationId, IdeationAnalysisBaseRefKind, Project, ProjectId,
};
use crate::error::AppError;

pub(super) fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should spawn");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub(super) fn init_workspace() -> (tempfile::TempDir, AgentConversationWorkspace, Project) {
    let repo = tempfile::TempDir::new().expect("repo tempdir");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "RalphX Test"]);
    std::fs::write(repo.path().join("README.md"), "base\n").expect("write base file");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let base_sha = git(repo.path(), &["rev-parse", "HEAD"]);

    let project_id = ProjectId::new();
    let mut project = Project::new(
        "Review pagination".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    project.id = project_id.clone();
    project.base_branch = Some("main".to_string());
    let workspace = AgentConversationWorkspace::new(
        ChatConversationId::new(),
        project_id,
        AgentConversationWorkspaceMode::Edit,
        IdeationAnalysisBaseRefKind::ProjectDefault,
        "main".to_string(),
        Some("Project default (main)".to_string()),
        Some(base_sha),
        "ralphx/test/review-pagination".to_string(),
        repo.path().to_string_lossy().to_string(),
    );
    (repo, workspace, project)
}

#[tokio::test]
async fn file_inventory_pages_every_path_and_normalizes_untracked_source() {
    let (repo, workspace, project) = init_workspace();
    for index in 0..121 {
        std::fs::write(repo.path().join(format!("file-{index:03}.txt")), "new\n")
            .expect("write untracked file");
    }

    let first = list_workspace_review_files(&workspace, &project, None, Some(50))
        .await
        .expect("first file page");
    assert_eq!(first.files.len(), 50);
    assert_eq!(first.total_count, 121);
    assert!(first.files.iter().all(|file| {
        file.status == "added"
            && file.sources == vec![AgentWorkspaceReviewDiffSource::Unstaged.as_str()]
    }));

    let second = list_workspace_review_files(
        &workspace,
        &project,
        first.next_cursor.as_deref(),
        Some(100),
    )
    .await
    .expect("second file page");
    assert_eq!(second.offset, 50);
    assert_eq!(second.files.len(), 71);
    assert!(second.next_cursor.is_none());
    assert_eq!(second.files.last().expect("last file").path, "file-120.txt");
}

#[tokio::test]
async fn file_inventory_preserves_rename_status_and_paths_with_spaces() {
    let (repo, mut workspace, project) = init_workspace();
    std::fs::write(repo.path().join("old name.txt"), "rename me\n").expect("write old file");
    git(repo.path(), &["add", "old name.txt"]);
    git(repo.path(), &["commit", "-m", "add rename source"]);
    workspace.base_commit = Some(git(repo.path(), &["rev-parse", "HEAD"]));
    git(repo.path(), &["mv", "old name.txt", "new name.txt"]);

    let page = list_workspace_review_files(&workspace, &project, None, None)
        .await
        .expect("renamed file inventory");
    assert_eq!(page.files.len(), 1);
    assert_eq!(page.files[0].path, "new name.txt");
    assert_eq!(page.files[0].status, "renamed");
    assert_eq!(
        page.files[0].sources,
        vec![AgentWorkspaceReviewDiffSource::Staged.as_str()]
    );
}

#[tokio::test]
async fn file_inventory_preserves_paths_with_tabs_and_newlines() {
    let (repo, workspace, project) = init_workspace();
    for path in ["tab\tname.txt", "line\nbreak.txt"] {
        std::fs::write(repo.path().join(path), "special path\n").expect("write special path");
        git(repo.path(), &["add", path]);
    }

    let page = list_workspace_review_files(&workspace, &project, None, None)
        .await
        .expect("special paths should remain selectable");

    assert_eq!(page.total_count, 2);
    assert!(page.files.iter().any(|file| file.path == "tab\tname.txt"));
    assert!(page.files.iter().any(|file| file.path == "line\nbreak.txt"));
    assert!(page
        .files
        .iter()
        .all(|file| { file.sources == vec![AgentWorkspaceReviewDiffSource::Staged.as_str()] }));
}

#[tokio::test]
async fn diff_continuation_repeats_active_hunk_anchor() {
    let (repo, workspace, project) = init_workspace();
    let old = (0..40)
        .map(|index| format!("old-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(repo.path().join("README.md"), format!("{old}\n"))
        .expect("write staged baseline");
    git(repo.path(), &["add", "README.md"]);
    let new = (0..40)
        .map(|index| format!("new-{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(repo.path().join("README.md"), format!("{new}\n"))
        .expect("write unstaged change");

    let first = get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("README.md"),
        Some("unstaged"),
        Some(10),
    )
    .await
    .expect("first diff page");
    assert_eq!(first.hunk_anchors.len(), 1);
    let second = get_workspace_review_diff_page(
        &workspace,
        &project,
        first.next_cursor.as_deref(),
        None,
        None,
        Some(10),
    )
    .await
    .expect("second diff page");
    assert_eq!(second.page.offset, 10);
    assert_eq!(second.hunk_anchors, first.hunk_anchors);
}

#[tokio::test]
async fn diff_page_rejects_a_single_row_that_exceeds_the_response_budget() {
    let (repo, workspace, project) = init_workspace();
    std::fs::write(repo.path().join("huge-line.txt"), "x".repeat(600_000))
        .expect("write huge one-line file");

    let error = get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("huge-line.txt"),
        Some("unstaged"),
        Some(2),
    )
    .await
    .expect_err("oversized serialized pages must fail closed");

    assert!(matches!(error, AppError::Validation(_)));
    assert!(error.to_string().contains("response size"));
}

#[tokio::test]
async fn file_cursor_rejects_staging_only_source_repartition() {
    let (repo, workspace, project) = init_workspace();
    std::fs::write(repo.path().join("README.md"), "changed\n").expect("modify tracked file");
    std::fs::write(repo.path().join("second.txt"), "untracked\n").expect("write untracked file");

    let first = list_workspace_review_files(&workspace, &project, None, Some(1))
        .await
        .expect("first file page");
    let cursor = first.next_cursor.expect("continuation cursor");
    git(repo.path(), &["add", "README.md"]);

    let error = list_workspace_review_files(&workspace, &project, Some(&cursor), Some(1))
        .await
        .expect_err("staging-only repartition must stale the cursor");
    assert!(matches!(error, AppError::Conflict(_)));
}

#[cfg(unix)]
#[tokio::test]
async fn unstaged_review_diff_rejects_symlink_escape() {
    let (repo, workspace, project) = init_workspace();
    let outside = tempfile::TempDir::new().expect("outside tempdir");
    std::fs::write(outside.path().join("secret.txt"), "secret\n").expect("write outside file");
    std::os::unix::fs::symlink(
        outside.path().join("secret.txt"),
        repo.path().join("escape.txt"),
    )
    .expect("create escaping symlink");

    let error = get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("escape.txt"),
        Some("unstaged"),
        Some(20),
    )
    .await
    .expect_err("escaping symlink must be rejected");
    assert!(matches!(error, AppError::Validation(_)));
}

#[tokio::test]
async fn exact_file_diff_exposes_hunks_beyond_compact_anchor_cap() {
    let (repo, mut workspace, project) = init_workspace();
    let baseline = (0..601)
        .flat_map(|index| {
            let mut block = vec![format!("old-{index}")];
            block.extend((0..8).map(|line| format!("context-{index}-{line}")));
            block
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(repo.path().join("many.txt"), format!("{baseline}\n"))
        .expect("write baseline file");
    git(repo.path(), &["add", "many.txt"]);
    git(repo.path(), &["commit", "-m", "many hunk baseline"]);
    workspace.base_commit = Some(git(repo.path(), &["rev-parse", "HEAD"]));
    let changed = baseline.replace("old-", "new-");
    std::fs::write(repo.path().join("many.txt"), format!("{changed}\n"))
        .expect("write changed file");

    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("resolve target")
        .expect("workspace target");
    let anchors = all_hunk_anchors_for_file(
        &target,
        "many.txt",
        AgentWorkspaceReviewDiffSource::Unstaged,
    )
    .expect("load exact hunks");

    assert!(anchors.len() > 600);
    assert!(anchors[600].hunk_header.starts_with("@@"));
}

#[test]
fn cursor_validation_rejects_wrong_kind_and_out_of_bounds_fields() {
    let cursor = ReviewDiffCursor {
        version: 1,
        kind: ReviewDiffCursorKind::Files,
        target_scope: "workspace_delta".to_string(),
        target_fingerprint: "a".repeat(REVIEW_FINGERPRINT_CHARS),
        source_fingerprint: "b".repeat(REVIEW_FINGERPRINT_CHARS),
        offset: 1,
        path: None,
        source: None,
    };
    let encoded = encode_cursor(&cursor).expect("encode cursor");
    assert!(decode_cursor(&encoded, ReviewDiffCursorKind::Diff).is_err());

    let oversized = ReviewDiffCursor {
        offset: REVIEW_CURSOR_MAX_OFFSET + 1,
        ..cursor
    };
    let encoded = encode_cursor(&oversized).expect("encode oversized cursor");
    assert!(decode_cursor(&encoded, ReviewDiffCursorKind::Files).is_err());
    assert!(decode_cursor("not-base64!", ReviewDiffCursorKind::Files).is_err());
}

#[test]
fn cursor_validation_accepts_complete_diff_cursor_and_rejects_invalid_shapes() {
    let cursor = ReviewDiffCursor {
        version: 1,
        kind: ReviewDiffCursorKind::Diff,
        target_scope: "workspace_delta".to_string(),
        target_fingerprint: "a".repeat(REVIEW_FINGERPRINT_CHARS),
        source_fingerprint: "b".repeat(REVIEW_FINGERPRINT_CHARS),
        offset: 4,
        path: Some("src/lib.rs".to_string()),
        source: Some(AgentWorkspaceReviewDiffSource::Unstaged),
    };
    let encoded = encode_cursor(&cursor).expect("encode complete cursor");
    let decoded = decode_cursor(&encoded, ReviewDiffCursorKind::Diff).expect("decode cursor");
    assert_eq!(decoded.offset, 4);
    assert_eq!(decoded.path.as_deref(), Some("src/lib.rs"));

    for invalid in [
        ReviewDiffCursor {
            path: None,
            ..cursor.clone()
        },
        ReviewDiffCursor {
            source: None,
            ..cursor.clone()
        },
        ReviewDiffCursor {
            target_fingerprint: "short".to_string(),
            ..cursor.clone()
        },
        ReviewDiffCursor {
            path: Some(" ".to_string()),
            ..cursor.clone()
        },
    ] {
        let encoded = encode_cursor(&invalid).expect("encode invalid cursor shape");
        assert!(decode_cursor(&encoded, ReviewDiffCursorKind::Diff).is_err());
    }

    let file_cursor = ReviewDiffCursor {
        kind: ReviewDiffCursorKind::Files,
        path: Some("README.md".to_string()),
        source: None,
        ..cursor
    };
    let encoded = encode_cursor(&file_cursor).expect("encode invalid file cursor");
    assert!(decode_cursor(&encoded, ReviewDiffCursorKind::Files).is_err());
}

#[test]
fn cursor_limits_and_source_parser_enforce_public_input_bounds() {
    assert_eq!(
        bounded_limit(None, 10, 20, "test").expect("default limit"),
        10
    );
    assert_eq!(
        bounded_limit(Some(20), 10, 20, "test").expect("max limit"),
        20
    );
    assert!(bounded_limit(Some(0), 10, 20, "test").is_err());
    assert!(bounded_limit(Some(21), 10, 20, "test").is_err());
    assert!(validate_path_bound(&"x".repeat(512)).is_ok());
    assert!(validate_path_bound(&"x".repeat(513)).is_err());

    for value in ["selected_source", "committed", "staged", "unstaged"] {
        assert!(value.parse::<AgentWorkspaceReviewDiffSource>().is_ok());
    }
    assert!("unknown".parse::<AgentWorkspaceReviewDiffSource>().is_err());
}

#[tokio::test]
async fn file_inventory_merges_committed_staged_and_unstaged_sources() {
    let (repo, mut workspace, project) = init_workspace();
    std::fs::write(repo.path().join("README.md"), "committed\n").expect("write committed change");
    git(repo.path(), &["add", "README.md"]);
    git(repo.path(), &["commit", "-m", "committed change"]);
    std::fs::write(repo.path().join("README.md"), "staged\n").expect("write staged change");
    git(repo.path(), &["add", "README.md"]);
    std::fs::write(repo.path().join("scratch.txt"), "unstaged\n").expect("write untracked change");
    workspace.base_commit = Some(git(repo.path(), &["rev-parse", "HEAD^"]));

    let page = list_workspace_review_files(&workspace, &project, None, None)
        .await
        .expect("inventory across source layers");
    let readme = page
        .files
        .iter()
        .find(|file| file.path == "README.md")
        .expect("README should be present");
    assert_eq!(readme.sources, vec!["committed", "staged"]);
    assert!(page.files.iter().any(|file| {
        file.path == "scratch.txt" && file.sources == vec!["unstaged"] && file.status == "added"
    }));
}

#[tokio::test]
async fn workspace_review_retains_two_file_target_after_base_rebase() {
    let (repo, mut workspace, project) = init_workspace();
    git(repo.path(), &["checkout", "-b", "feature/rebased-review"]);
    std::fs::write(repo.path().join("first.rs"), "pub fn first() {}\n")
        .expect("first feature file should be written");
    std::fs::write(repo.path().join("second.rs"), "pub fn second() {}\n")
        .expect("second feature file should be written");
    git(repo.path(), &["add", "first.rs", "second.rs"]);
    git(repo.path(), &["commit", "-m", "two feature files"]);
    git(repo.path(), &["checkout", "main"]);
    std::fs::write(repo.path().join("base.txt"), "advanced base\n")
        .expect("base advance should be written");
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "advance base"]);
    let updated_base = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["checkout", "feature/rebased-review"]);
    git(repo.path(), &["rebase", "main"]);
    workspace.base_commit = Some(updated_base.clone());
    workspace.branch_name = "feature/rebased-review".to_string();

    let target = resolve_review_target(&workspace, &project)
        .await
        .expect("rebased workspace target should resolve")
        .expect("rebased workspace should retain its feature delta");

    assert_eq!(target.base_sha.as_deref(), Some(updated_base.as_str()));
    assert_eq!(target.review_packet.summary.files_changed, 2);
    assert_eq!(
        target
            .review_packet
            .changed_files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["first.rs", "second.rs"]
    );
}

#[tokio::test]
async fn diff_page_rejects_invalid_first_page_and_cursor_offsets() {
    let (repo, workspace, project) = init_workspace();
    std::fs::write(repo.path().join("README.md"), "changed\n").expect("modify tracked file");

    assert!(get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        None,
        Some("unstaged"),
        None
    )
    .await
    .is_err());
    assert!(get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("README.md"),
        None,
        None
    )
    .await
    .is_err());

    let first = get_workspace_review_diff_page(
        &workspace,
        &project,
        None,
        Some("README.md"),
        Some("unstaged"),
        Some(1),
    )
    .await
    .expect("first diff page");
    let cursor = first.next_cursor.expect("continuation cursor");
    assert!(get_workspace_review_diff_page(
        &workspace,
        &project,
        Some(&cursor),
        Some("README.md"),
        None,
        Some(1),
    )
    .await
    .is_err());

    let mut decoded = decode_cursor(&cursor, ReviewDiffCursorKind::Diff).expect("decode cursor");
    decoded.offset = 99;
    let out_of_range = encode_cursor(&decoded).expect("encode out of range cursor");
    let error = get_workspace_review_diff_page(
        &workspace,
        &project,
        Some(&out_of_range),
        None,
        None,
        Some(1),
    )
    .await
    .expect_err("out of range cursor must fail");
    assert!(matches!(error, AppError::Validation(_)));
}

#[test]
fn source_scope_validation_fails_closed() {
    assert!(validate_source_for_target(
        AgentWorkspaceReviewDiffSource::SelectedSource,
        AgentWorkspaceReviewTargetScope::WorkspaceDelta,
    )
    .is_err());
    assert!(validate_source_for_target(
        AgentWorkspaceReviewDiffSource::Staged,
        AgentWorkspaceReviewTargetScope::SelectedSource,
    )
    .is_err());
}
