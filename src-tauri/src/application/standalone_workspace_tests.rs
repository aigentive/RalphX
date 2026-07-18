use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tempfile::TempDir;

use crate::application::standalone_workspace::{
    create_workspace, remove_workspace_if_present, resolve_workspace, standalone_workspace_path,
    standalone_workspaces_root, sweep_orphaned_standalone_workspaces,
};
use crate::domain::entities::{ChatConversation, ProjectId};
use crate::domain::repositories::ChatConversationRepository;
use crate::error::AppError;
use crate::infrastructure::memory::MemoryChatConversationRepository;

fn new_conversation_id() -> String {
    ChatConversation::new_project(ProjectId::from_string(
        "standalone-workspace-fixture".into(),
    ))
    .id
    .as_str()
}

fn canonical_test_dir(path: &Path) -> PathBuf {
    let validated = crate::utils::path_safety::validate_absolute_non_root_path(
        path,
        "standalone workspace test root",
    )
    .expect("validated absolute test root");
    // codeql[rust/path-injection]
    validated.canonicalize().expect("canonicalize test root")
}

#[test]
fn create_workspace_is_idempotent_and_returns_same_path_twice() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let conversation_id = new_conversation_id();

    let first = create_workspace(app_data_dir.path(), &conversation_id).expect("first create");
    let second = create_workspace(app_data_dir.path(), &conversation_id).expect("second create");

    assert_eq!(first, second, "ensure_workspace must be idempotent");
    assert!(first.is_dir(), "workspace must exist on disk");

    let root = standalone_workspaces_root(
        &app_data_dir
            .path()
            .canonicalize()
            .expect("canonicalize app data dir"),
    );
    let entries: Vec<_> = fs::read_dir(&root)
        .expect("read workspaces root")
        .collect::<Result<Vec<_>, _>>()
        .expect("valid dir entries");
    assert_eq!(
        entries.len(),
        1,
        "two ensure_workspace calls for the same conversation must not create two directories"
    );
}

#[test]
fn create_workspace_creates_missing_process_owned_app_data_root() {
    let app_data_parent = TempDir::new().expect("temp app data parent");
    let app_data_dir = app_data_parent.path().join("new-app-data");
    let conversation_id = new_conversation_id();

    let workspace = create_workspace(&app_data_dir, &conversation_id)
        .expect("create workspace under a missing app data root");
    let canonical_app_data_dir = canonical_test_dir(&app_data_dir);

    assert!(
        workspace.starts_with(&canonical_app_data_dir),
        "workspace must stay under the newly created process-owned app data root"
    );
}

#[test]
fn create_workspace_hashes_conversation_id_component() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let conversation_id = "very-distinctive-raw-conversation-id-marker";

    let workspace = create_workspace(app_data_dir.path(), conversation_id)
        .expect("create_workspace with a non-UUID id");

    let workspace_display = workspace.to_string_lossy();
    assert!(
        !workspace_display.contains(conversation_id),
        "the raw conversation id must never appear in the workspace path: {workspace_display}"
    );
    let component_name = workspace
        .file_name()
        .and_then(|name| name.to_str())
        .expect("workspace has a directory name");
    assert!(
        component_name.starts_with("conversation-"),
        "workspace directory name must use the hashed component prefix: {component_name}"
    );
}

#[test]
fn create_workspace_is_safe_under_concurrent_calls() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let conversation_id = new_conversation_id();
    let app_data_dir_path = app_data_dir.path().to_path_buf();

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let app_data_dir_path = app_data_dir_path.clone();
            let conversation_id = conversation_id.clone();
            std::thread::spawn(move || {
                create_workspace(&app_data_dir_path, &conversation_id)
                    .expect("concurrent create_workspace must succeed")
            })
        })
        .collect();

    let mut resolved_paths = handles
        .into_iter()
        .map(|handle| handle.join().expect("worker thread must not panic"))
        .collect::<Vec<_>>();
    resolved_paths.dedup();

    assert_eq!(
        resolved_paths.len(),
        1,
        "all concurrent callers must resolve to the same workspace path"
    );
    assert!(resolved_paths[0].is_dir());
}

#[test]
fn create_workspace_path_traversal_conversation_id_stays_contained() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let malicious_id = "../../../evil";

    let workspace = create_workspace(app_data_dir.path(), malicious_id)
        .expect("hashing makes the traversal payload inert");

    let canonical_root = standalone_workspaces_root(&canonical_test_dir(app_data_dir.path()));
    assert!(
        workspace.starts_with(&canonical_root),
        "workspace path must stay under the standalone workspaces root even for a \
         path-traversal conversation id: {workspace:?} vs root {canonical_root:?}"
    );
    assert!(
        !workspace
            .components()
            .any(|component| component.as_os_str() == ".."),
        "resolved workspace path must not contain a literal .. component"
    );
}

#[test]
fn create_workspace_absolute_conversation_id_stays_contained() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let absolute_id = "/tmp/absolute-conversation-id";

    let workspace = create_workspace(app_data_dir.path(), absolute_id)
        .expect("hashing makes an absolute conversation id inert");

    let canonical_root = standalone_workspaces_root(&canonical_test_dir(app_data_dir.path()));
    assert!(
        workspace.starts_with(&canonical_root),
        "an absolute conversation id must stay under the app-owned root"
    );
    assert!(
        !workspace.to_string_lossy().contains(absolute_id),
        "the absolute conversation id must not appear in the workspace path"
    );
}

#[test]
fn create_workspace_rejects_app_data_path_with_parent_components() {
    let app_data_parent = TempDir::new().expect("temp app data parent");
    let nested = app_data_parent.path().join("nested");
    assert!(nested.starts_with(app_data_parent.path()));
    // codeql[rust/path-injection]
    fs::create_dir(&nested).expect("create nested app data segment");
    let unsafe_app_data_dir = nested.join("..");

    let result = create_workspace(&unsafe_app_data_dir, "conversation-id");

    assert!(
        result.is_err(),
        "an app-data path with traversal components must be rejected, got: {result:?}"
    );
}

#[test]
fn create_workspace_rejects_symlinked_root_escape() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let outside = TempDir::new().expect("outside target dir");
    let root = standalone_workspaces_root(app_data_dir.path());
    assert!(root.starts_with(app_data_dir.path()));
    // codeql[rust/path-injection]
    symlink(outside.path(), &root).expect("create symlinked workspaces root");
    let conversation_id = "symlink-root-escape";

    let result = create_workspace(app_data_dir.path(), conversation_id);

    assert!(
        result.is_err(),
        "a symlinked workspaces root must be rejected, got: {result:?}"
    );
    let escaped_workspace = standalone_workspace_path(outside.path(), conversation_id);
    assert!(escaped_workspace.starts_with(outside.path()));
    // codeql[rust/path-injection]
    assert!(
        !escaped_workspace.exists(),
        "workspace creation must not follow the root symlink outside app data"
    );
}

#[test]
fn create_workspace_returns_typed_error_when_root_segment_is_blocked_by_a_file() {
    let temp = TempDir::new().expect("temp dir");
    let blocked_app_data_dir = temp.path().join("blocked-app-data");
    fs::write(&blocked_app_data_dir, b"not a directory").expect("write blocking file");

    let result = create_workspace(&blocked_app_data_dir, "any-conversation-id");

    assert!(
        result.is_err(),
        "workspace creation must fail closed when the app-owned root cannot be created, \
         got: {result:?}"
    );
}

#[test]
fn remove_workspace_accepts_hash_inert_traversal_id() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let conversation_id = "../../../conversation-to-remove";
    let workspace = create_workspace(app_data_dir.path(), conversation_id)
        .expect("create workspace for traversal-shaped id");

    remove_workspace_if_present(app_data_dir.path(), conversation_id)
        .expect("remove hash-derived workspace");

    let canonical_app_data_dir = canonical_test_dir(app_data_dir.path());
    assert!(workspace.starts_with(&canonical_app_data_dir));
    // codeql[rust/path-injection]
    assert!(
        !workspace.exists(),
        "the contained hash-derived workspace must be removed"
    );
}

#[test]
fn remove_workspace_missing_app_data_root_is_a_noop() {
    let app_data_parent = TempDir::new().expect("temp app data parent");
    let missing_app_data_dir = app_data_parent.path().join("missing-app-data");
    remove_workspace_if_present(&missing_app_data_dir, "missing-conversation")
        .expect("missing app-data removal must be a no-op");
}

#[test]
fn resolve_workspace_missing_app_data_root_returns_typed_missing_error() {
    let app_data_parent = TempDir::new().expect("temp app data parent");
    let missing_app_data_dir = app_data_parent.path().join("missing-app-data");
    let result = resolve_workspace(&missing_app_data_dir, "missing-conversation");
    assert!(
        matches!(result, Err(AppError::StandaloneWorkspaceMissing { .. })),
        "missing app-data root must preserve the typed workspace-missing error, got: {result:?}"
    );
}

#[test]
fn remove_workspace_rejects_symlink_escape() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let outside = TempDir::new().expect("outside target dir");
    let conversation_id = "symlinked-workspace";
    let workspace = create_workspace(app_data_dir.path(), conversation_id)
        .expect("create workspace before replacing it with a symlink");
    let canonical_root = standalone_workspaces_root(&canonical_test_dir(app_data_dir.path()));
    assert!(workspace.starts_with(&canonical_root));
    // codeql[rust/path-injection]
    fs::remove_dir_all(&workspace).expect("remove original contained workspace");
    assert!(workspace.starts_with(&canonical_root));
    // codeql[rust/path-injection]
    symlink(outside.path(), &workspace).expect("replace workspace with outside symlink");
    let sentinel = outside.path().join("sentinel.txt");
    assert!(sentinel.starts_with(outside.path()));
    // codeql[rust/path-injection]
    fs::write(&sentinel, b"must survive").expect("write outside sentinel");

    let result = remove_workspace_if_present(app_data_dir.path(), conversation_id);

    assert!(
        result.is_err(),
        "workspace removal must reject a symlink escape, got: {result:?}"
    );
    assert!(sentinel.starts_with(outside.path()));
    // codeql[rust/path-injection]
    assert!(
        sentinel.is_file(),
        "outside symlink target must remain intact"
    );
}

async fn seed_conversation(repo: &Arc<dyn ChatConversationRepository>) -> String {
    let conversation =
        ChatConversation::new_project(ProjectId::from_string("standalone-sweep-fixture".into()));
    let conversation_id = conversation.id.as_str();
    repo.create(conversation)
        .await
        .expect("seed live conversation row");
    conversation_id
}

#[tokio::test]
async fn sweep_removes_workspace_with_no_matching_conversation_row() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let orphan_conversation_id = new_conversation_id();
    let workspace = create_workspace(app_data_dir.path(), &orphan_conversation_id)
        .expect("create orphan workspace");
    assert!(workspace.is_dir());

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(summary.removed, 1, "orphaned workspace must be removed");
    assert_eq!(summary.retained, 0);
    assert!(
        !workspace.exists(),
        "orphaned workspace directory must be deleted from disk"
    );
}

#[tokio::test]
async fn sweep_retains_workspace_with_live_conversation_row() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let conversation_id = seed_conversation(&repo).await;
    let workspace =
        create_workspace(app_data_dir.path(), &conversation_id).expect("create live workspace");

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(summary.removed, 0);
    assert_eq!(
        summary.retained, 1,
        "live conversation's workspace must survive the sweep"
    );
    assert!(
        workspace.is_dir(),
        "workspace directory must remain on disk when its conversation still exists"
    );
}

/// Archiving a Standalone conversation must not delete its workspace: the sweep keys
/// purely on DB-row existence, and archived conversations still have a DB row. This also
/// proves restore (un-archiving) never needed to "recreate" anything the sweep destroyed.
#[tokio::test]
async fn sweep_retains_workspace_for_archived_conversation_with_live_db_row() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let mut conversation =
        ChatConversation::new_project(ProjectId::from_string("standalone-archive-fixture".into()));
    conversation.archived_at = Some(chrono::Utc::now());
    let conversation_id = conversation.id.as_str();
    repo.create(conversation)
        .await
        .expect("seed archived conversation row");
    let workspace = create_workspace(app_data_dir.path(), &conversation_id)
        .expect("create workspace for archived conversation");

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(
        summary.removed, 0,
        "an archived conversation still has a DB row and must not be swept as orphaned"
    );
    assert!(
        workspace.is_dir(),
        "workspace for an archived-but-existing conversation must survive the sweep"
    );

    // Restore: clearing archived_at must not require recreating anything, since the
    // workspace was never deleted.
    let restored = resolve_workspace(app_data_dir.path(), &conversation_id)
        .expect("workspace resolution after restore must keep working");
    assert_eq!(restored, workspace);
}

#[tokio::test]
async fn sweep_does_not_follow_or_delete_a_symlinked_entry() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let root = standalone_workspaces_root(app_data_dir.path());
    fs::create_dir_all(&root).expect("create workspaces root");

    let outside = TempDir::new().expect("outside target dir");
    let sentinel_file = outside.path().join("sentinel.txt");
    fs::write(&sentinel_file, b"must survive the sweep").expect("write sentinel file");

    let symlink_entry = root.join("conversation-deadbeefdeadbeef");
    symlink(outside.path(), &symlink_entry).expect("create symlinked workspace entry");

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(
        summary.removed, 0,
        "a symlinked entry must never be deleted by the sweep"
    );
    assert!(
        symlink_entry.exists(),
        "the symlink entry itself must remain untouched"
    );
    assert!(
        sentinel_file.exists(),
        "the sweep must never delete content outside the standalone workspaces root through a symlink"
    );
}

#[tokio::test]
async fn sweep_does_not_follow_a_symlinked_workspaces_root() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let outside_app_data_dir = TempDir::new().expect("outside app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let orphan_conversation_id = new_conversation_id();
    let outside_workspace = create_workspace(outside_app_data_dir.path(), &orphan_conversation_id)
        .expect("create workspace outside the swept app data root");
    let outside_root = standalone_workspaces_root(&canonical_test_dir(outside_app_data_dir.path()));
    let symlinked_root = standalone_workspaces_root(app_data_dir.path());
    assert!(symlinked_root.starts_with(app_data_dir.path()));
    // codeql[rust/path-injection]
    symlink(&outside_root, &symlinked_root).expect("symlink workspaces root outside app data");

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(
        summary,
        Default::default(),
        "a symlinked workspaces root must be skipped without inspecting its target"
    );
    assert!(outside_workspace.starts_with(&outside_root));
    // codeql[rust/path-injection]
    assert!(
        outside_workspace.is_dir(),
        "the sweep must not remove a workspace through a symlinked root"
    );
}

#[tokio::test]
async fn sweep_skips_unreadable_manifest_without_blocking_valid_neighbor_removal() {
    let app_data_dir = TempDir::new().expect("temp app data dir");
    let repo: Arc<dyn ChatConversationRepository> =
        Arc::new(MemoryChatConversationRepository::new());
    let root = standalone_workspaces_root(app_data_dir.path());
    fs::create_dir_all(&root).expect("create workspaces root");

    // Neighbor A: corrupt/unreadable manifest — must be preserved (can't prove orphaned).
    let corrupt_entry = root.join("conversation-corruptcorrupt");
    fs::create_dir_all(&corrupt_entry).expect("create corrupt entry dir");
    fs::write(corrupt_entry.join("manifest.json"), b"not valid json")
        .expect("write corrupt manifest");

    // Neighbor B: valid manifest, no matching DB row — must be removed.
    let orphan_conversation_id = new_conversation_id();
    let valid_orphan_workspace = create_workspace(app_data_dir.path(), &orphan_conversation_id)
        .expect("create valid orphan workspace");

    let summary =
        sweep_orphaned_standalone_workspaces(app_data_dir.path(), Arc::clone(&repo)).await;

    assert_eq!(
        summary.removed, 1,
        "the valid orphaned neighbor must still be removed"
    );
    assert!(
        summary.skipped >= 1,
        "the unreadable-manifest entry must be counted as skipped, not removed"
    );
    assert!(
        corrupt_entry.is_dir(),
        "an entry with an unreadable/corrupt manifest must not be deleted"
    );
    assert!(
        !valid_orphan_workspace.exists(),
        "a valid orphaned neighbor must still be deleted despite the unreadable sibling"
    );
}
