use std::path::Path;
use std::process::Command;

use chrono::{Duration, TimeZone, Utc};

use crate::application::agent_runtime_context::branch_status::{
    parse_porcelain_counts, render_branch_status, BranchStatusCache, BranchStatusSnapshot,
};
use crate::domain::services::{PrMergeStateStatus, PrMergeableState, PrStatus, PrSyncState};

fn run_git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cold_branch_cache_is_explicitly_unknown_without_touching_the_workspace() {
    let cache = BranchStatusCache::default();
    let rendered = render_branch_status(
        &cache,
        Path::new("/path/that/must/not/be/probed"),
        Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        Duration::minutes(5),
    );

    assert!(rendered.contains("<dirty state=\"unknown\"/>"));
    assert!(rendered.contains("<base state=\"unknown\"/>"));
}

#[test]
fn warm_and_stale_branch_snapshots_keep_counts_and_observation_time() {
    let cache = BranchStatusCache::default();
    let observed_at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    cache.record(
        Path::new("/tmp/runtime-branch-status"),
        BranchStatusSnapshot {
            dirty_known: true,
            staged: 2,
            unstaged: 3,
            untracked: 4,
            dirty_as_of: observed_at,
            behind_base: Some(5),
            ahead_of_base: Some(6),
            base_ref: Some("main<&".to_string()),
            base_relation: Some("diverged".to_string()),
            base_as_of: Some(observed_at),
        },
    );

    let fresh = render_branch_status(
        &cache,
        Path::new("/tmp/runtime-branch-status"),
        observed_at + Duration::seconds(30),
        Duration::minutes(5),
    );
    assert!(fresh.contains("staged=\"2\""));
    assert!(fresh.contains("behind=\"5\""));
    assert!(fresh.contains("base_ref=\"main&lt;&amp;\""));
    assert!(!fresh.contains("stale=\"true\""));

    let stale = render_branch_status(
        &cache,
        Path::new("/tmp/runtime-branch-status"),
        observed_at + Duration::minutes(10),
        Duration::minutes(5),
    );
    assert!(stale.contains("stale=\"true\""));
    assert!(stale.contains("age_seconds=\"600\""));
}

#[test]
fn porcelain_counts_distinguish_staged_unstaged_and_untracked_entries() {
    let counts = parse_porcelain_counts("M  staged.rs\n M unstaged.rs\nMM both.rs\n?? new.rs\n");

    assert_eq!(counts.staged, 2);
    assert_eq!(counts.unstaged, 2);
    assert_eq!(counts.untracked, 1);
}

#[test]
fn pr_observation_records_relation_without_inventing_commit_counts() {
    let cache = BranchStatusCache::default();
    let observed_at = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
    cache.observe_pr_sync(
        Path::new("/tmp/pr-observed-workspace"),
        &PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Behind),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature/runtime".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("head".to_string()),
            base_ref_oid: Some("base".to_string()),
        },
        observed_at,
    );

    let snapshot = cache
        .snapshot(Path::new("/tmp/pr-observed-workspace"))
        .expect("PR observation should populate cache");
    assert_eq!(snapshot.base_relation.as_deref(), Some("behind"));
    assert_eq!(snapshot.base_ref.as_deref(), Some("main"));
    assert_eq!(snapshot.behind_base, None);
    assert_eq!(snapshot.ahead_of_base, None);
    assert!(cache.refresh_due(
        Path::new("/tmp/pr-observed-workspace"),
        observed_at + Duration::seconds(10),
        Duration::seconds(30),
    ));

    let rendered = render_branch_status(
        &cache,
        Path::new("/tmp/pr-observed-workspace"),
        observed_at + Duration::seconds(10),
        Duration::minutes(5),
    );
    assert!(rendered.contains("<dirty state=\"unknown\"/>"));
    assert!(rendered.contains("relation=\"behind\""));
    assert!(!rendered.contains("behind=\"\""));

    cache.observe_pr_sync(
        Path::new("/tmp/pr-observed-workspace"),
        &PrSyncState {
            status: PrStatus::Open,
            merge_state_status: Some(PrMergeStateStatus::Clean),
            mergeable: Some(PrMergeableState::Mergeable),
            is_draft: false,
            head_ref_name: "feature/runtime".to_string(),
            base_ref_name: "stale-base".to_string(),
            head_ref_oid: Some("old-head".to_string()),
            base_ref_oid: Some("old-base".to_string()),
        },
        observed_at - Duration::seconds(1),
    );
    let after_stale_observation = cache
        .snapshot(Path::new("/tmp/pr-observed-workspace"))
        .expect("newer observation should remain cached");
    assert_eq!(after_stale_observation.base_ref.as_deref(), Some("main"));
    assert_eq!(
        after_stale_observation.base_relation.as_deref(),
        Some("behind")
    );
}

#[tokio::test]
async fn local_refresh_reads_dirty_and_base_counts_without_a_remote() {
    let repo = tempfile::TempDir::new().expect("temp repo should be created");
    let path = repo.path();
    run_git(path, &["init", "-b", "main"]);
    run_git(
        path,
        &["config", "user.email", "runtime-context@example.invalid"],
    );
    run_git(path, &["config", "user.name", "Runtime Context Test"]);
    std::fs::write(path.join("shared.txt"), "initial\n").expect("initial file should write");
    run_git(path, &["add", "shared.txt"]);
    run_git(path, &["commit", "-m", "initial"]);
    run_git(path, &["switch", "-c", "feature"]);
    std::fs::write(path.join("feature.txt"), "feature\n").expect("feature file should write");
    run_git(path, &["add", "feature.txt"]);
    run_git(path, &["commit", "-m", "feature"]);
    run_git(path, &["switch", "main"]);
    std::fs::write(path.join("main.txt"), "main\n").expect("main file should write");
    run_git(path, &["add", "main.txt"]);
    run_git(path, &["commit", "-m", "main"]);
    run_git(path, &["switch", "feature"]);

    std::fs::write(path.join("feature.txt"), "feature changed\n")
        .expect("tracked file should update");
    std::fs::write(path.join("untracked.txt"), "new\n").expect("untracked file should write");

    let cache = BranchStatusCache::default();
    cache
        .refresh_local(path, Some("main"))
        .await
        .expect("local refresh should succeed without a remote");

    let snapshot = cache.snapshot(path).expect("refresh should populate cache");
    assert!(snapshot.dirty_known);
    assert_eq!(snapshot.unstaged, 1);
    assert_eq!(snapshot.untracked, 1);
    assert_eq!(snapshot.behind_base, Some(1));
    assert_eq!(snapshot.ahead_of_base, Some(1));
    assert_eq!(snapshot.base_relation.as_deref(), Some("diverged"));
}
