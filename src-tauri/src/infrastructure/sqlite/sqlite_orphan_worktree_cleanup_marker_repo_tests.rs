use chrono::{Duration, Utc};

use crate::domain::entities::ProjectId;
use crate::domain::repositories::{
    OrphanWorktreeCleanupMarker, OrphanWorktreeCleanupMarkerKey,
    OrphanWorktreeCleanupMarkerRepository,
};
use crate::testing::SqliteTestDb;

use super::SqliteOrphanWorktreeCleanupMarkerRepository;

fn marker_key() -> OrphanWorktreeCleanupMarkerKey {
    OrphanWorktreeCleanupMarkerKey {
        project_id: ProjectId::from_string("project-1".to_string()),
        worktree_path: "/tmp/ralphx-worktrees/project/agent-conversation-1".to_string(),
        branch_name: "ralphx/project/agent-1".to_string(),
        cleanup_status: "unsafe".to_string(),
        head_sha: Some("head-1".to_string()),
        target_ref: Some("origin/main".to_string()),
    }
}

fn setup_repo() -> (SqliteTestDb, SqliteOrphanWorktreeCleanupMarkerRepository) {
    let db = SqliteTestDb::new("sqlite_orphan_worktree_cleanup_marker_repo_tests");
    let repo = SqliteOrphanWorktreeCleanupMarkerRepository::from_shared(db.shared_conn());
    (db, repo)
}

#[tokio::test]
async fn orphan_cleanup_marker_matches_recent_same_state() {
    let (_db, repo) = setup_repo();
    let key = marker_key();

    repo.mark(OrphanWorktreeCleanupMarker {
        key: key.clone(),
        checked_at: Utc::now(),
    })
    .await
    .unwrap();

    assert!(repo
        .has_recent_marker(&key, Utc::now() - Duration::hours(1))
        .await
        .unwrap());
}

#[tokio::test]
async fn orphan_cleanup_marker_rejects_stale_or_changed_state() {
    let (_db, repo) = setup_repo();
    let key = marker_key();

    repo.mark(OrphanWorktreeCleanupMarker {
        key: key.clone(),
        checked_at: Utc::now() - Duration::hours(25),
    })
    .await
    .unwrap();

    assert!(!repo
        .has_recent_marker(&key, Utc::now() - Duration::hours(24))
        .await
        .unwrap());

    let mut changed_head = key.clone();
    changed_head.head_sha = Some("head-2".to_string());
    assert!(!repo
        .has_recent_marker(&changed_head, Utc::now() - Duration::hours(26))
        .await
        .unwrap());
}

#[tokio::test]
async fn orphan_cleanup_marker_clear_removes_all_statuses_for_worktree() {
    let (_db, repo) = setup_repo();
    let unsafe_key = marker_key();
    let mut dirty_key = unsafe_key.clone();
    dirty_key.cleanup_status = "dirty".to_string();
    dirty_key.target_ref = None;

    repo.mark(OrphanWorktreeCleanupMarker {
        key: unsafe_key.clone(),
        checked_at: Utc::now(),
    })
    .await
    .unwrap();
    repo.mark(OrphanWorktreeCleanupMarker {
        key: dirty_key.clone(),
        checked_at: Utc::now(),
    })
    .await
    .unwrap();

    repo.clear_for_worktree(
        &unsafe_key.project_id,
        &unsafe_key.worktree_path,
        &unsafe_key.branch_name,
    )
    .await
    .unwrap();

    let retry_after = Utc::now() - Duration::hours(1);
    assert!(!repo
        .has_recent_marker(&unsafe_key, retry_after)
        .await
        .unwrap());
    assert!(!repo
        .has_recent_marker(&dirty_key, retry_after)
        .await
        .unwrap());
}
