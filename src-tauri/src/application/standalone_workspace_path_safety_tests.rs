use std::fs;
use std::os::unix::fs::symlink;
use std::sync::Arc;

use tempfile::TempDir;

use crate::application::standalone_workspace::{
    create_workspace, standalone_workspace_path, standalone_workspaces_root,
    sweep_orphaned_standalone_workspaces,
};
use crate::domain::repositories::ChatConversationRepository;
use crate::infrastructure::memory::MemoryChatConversationRepository;

#[test]
fn create_workspace_rejects_symlink_at_hash_derived_workspace_path() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let outside = TempDir::new().expect("outside target dir");
    let root = standalone_workspaces_root(app_data_dir.path());
    // codeql[rust/path-injection]
    fs::create_dir_all(&root).expect("create standalone workspaces root");
    let workspace_path = standalone_workspace_path(&root, "symlinked-create-target");
    assert!(workspace_path.starts_with(&root));
    // codeql[rust/path-injection]
    symlink(outside.path(), &workspace_path).expect("create workspace symlink");
    let sentinel = outside.path().join("sentinel.txt");
    assert!(sentinel.starts_with(outside.path()));
    // codeql[rust/path-injection]
    fs::write(&sentinel, b"must survive").expect("write outside sentinel");

    let result = create_workspace(app_data_dir.path(), "symlinked-create-target");

    assert!(
        result.is_err(),
        "workspace creation must reject a symlink at the hash-derived destination, got: {result:?}"
    );
    // codeql[rust/path-injection]
    assert!(sentinel.is_file(), "outside symlink target must survive");
}

#[tokio::test]
async fn sweep_skips_symlinked_manifest_and_preserves_outside_target() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let outside = TempDir::new().expect("outside target dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let workspace = create_workspace(app_data_dir.path(), "symlinked-manifest")
        .expect("create candidate workspace");
    let manifest_path = workspace.join("manifest.json");
    assert!(manifest_path.starts_with(&workspace));
    // codeql[rust/path-injection]
    fs::remove_file(&manifest_path).expect("remove app-owned manifest");
    let outside_manifest = outside.path().join("outside-manifest.json");
    assert!(outside_manifest.starts_with(outside.path()));
    // codeql[rust/path-injection]
    fs::write(
        &outside_manifest,
        br#"{"conversationId":"not-authoritative","createdAt":"2026-07-18T00:00:00Z"}"#,
    )
    .expect("write outside manifest target");
    // codeql[rust/path-injection]
    symlink(&outside_manifest, &manifest_path).expect("replace manifest with outside symlink");

    let summary = sweep_orphaned_standalone_workspaces(app_data_dir.path(), repo).await;

    assert_eq!(
        summary.removed, 0,
        "symlinked manifest must never authorize deletion"
    );
    assert_eq!(
        summary.skipped, 1,
        "symlinked manifest must be skipped fail-closed"
    );
    // codeql[rust/path-injection]
    assert!(
        workspace.is_dir(),
        "workspace with symlinked manifest must survive"
    );
    // codeql[rust/path-injection]
    assert!(
        outside_manifest.is_file(),
        "outside manifest target must survive"
    );
}
