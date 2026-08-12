use super::*;

use crate::domain::services::github_service::{PrHealth, PrHealthCheck, PrStatus, PrSyncState};
use crate::tests::mock_github_service::MockGithubService;

const REPO: &str = "/tmp/pr-snapshot-hub-repo";

fn hub_github() -> (Arc<MockGithubService>, Arc<dyn GithubServiceTrait>) {
    let mock = Arc::new(MockGithubService::new());
    let service: Arc<dyn GithubServiceTrait> = Arc::clone(&mock) as Arc<dyn GithubServiceTrait>;
    (mock, service)
}

/// The point of the hub: N pollers on one repository inside one TTL window cost one GitHub read.
#[tokio::test]
async fn registered_prs_share_a_single_batched_read_within_one_window() {
    let hub = PrSnapshotHub::new();
    let (mock, github) = hub_github();
    for pr in [101, 102, 103] {
        hub.register(REPO, pr);
    }

    for pr in [101, 102, 103] {
        let snapshot = hub
            .get_snapshot(REPO, pr, &github, Path::new(REPO))
            .await
            .expect("registered PRs should resolve");
        assert_eq!(
            snapshot.sync_state.head_ref_oid.as_deref(),
            Some(format!("batched-head-{pr}").as_str()),
            "each poller must receive its own PR's state, not a neighbour's"
        );
    }

    let state = mock.state();
    assert_eq!(
        state.fetch_pr_status_snapshots_calls.len(),
        1,
        "three pollers in one TTL window must cost one batched read, got {:?}",
        state.fetch_pr_status_snapshots_calls
    );
    assert_eq!(
        state.fetch_pr_status_snapshots_calls[0],
        vec![101, 102, 103],
        "the single batch must cover every registered PR"
    );
    assert_eq!(
        state.fetch_pr_health_calls, 0,
        "a batched hit must not also pay a per-PR read"
    );
}

/// A PR the batch could not report must still resolve, via its own read.
#[tokio::test]
async fn a_pr_missing_from_the_batch_falls_back_to_a_single_per_pr_read() {
    let hub = PrSnapshotHub::new();
    let (mock, github) = hub_github();
    mock.state().fetch_pr_status_snapshots_known = Some(vec![101]);
    hub.register(REPO, 101);
    hub.register(REPO, 999);

    hub.get_snapshot(REPO, 999, &github, Path::new(REPO))
        .await
        .expect("an unbatched PR must still resolve");

    let state = mock.state();
    assert_eq!(state.fetch_pr_status_snapshots_calls.len(), 1);
    assert_eq!(
        state.fetch_pr_health_calls, 1,
        "exactly one fallback read for the PR the batch omitted"
    );
    assert_eq!(state.last_fetch_pr_health_number, Some(999));
}

/// An expired window must refetch. Nothing here serves a knowingly stale snapshot.
#[tokio::test]
async fn an_expired_window_triggers_exactly_one_more_batched_read() {
    let hub = PrSnapshotHub::new();
    let (mock, github) = hub_github();
    hub.register(REPO, 101);

    hub.get_snapshot(REPO, 101, &github, Path::new(REPO))
        .await
        .expect("first read");
    // Age the cached entry past any plausible TTL rather than sleeping for one.
    if let Some(mut entry) = hub.snapshots.get_mut(REPO) {
        entry.fetched_at = Instant::now() - Duration::from_secs(3600);
    }
    hub.get_snapshot(REPO, 101, &github, Path::new(REPO))
        .await
        .expect("second read after expiry");

    assert_eq!(mock.state().fetch_pr_status_snapshots_calls.len(), 2);
}

/// Concurrent pollers hitting a cold cache must collapse onto one refresh, not stampede.
#[tokio::test]
async fn concurrent_cold_reads_collapse_into_one_batched_refresh() {
    let hub = Arc::new(PrSnapshotHub::new());
    let (mock, github) = hub_github();
    for pr in [101, 102, 103, 104] {
        hub.register(REPO, pr);
    }

    let mut handles = Vec::new();
    for pr in [101, 102, 103, 104] {
        let hub = Arc::clone(&hub);
        let github = Arc::clone(&github);
        handles.push(tokio::spawn(async move {
            hub.get_snapshot(REPO, pr, &github, Path::new(REPO))
                .await
                .expect("concurrent read")
        }));
    }
    for handle in handles {
        handle.await.expect("task should not panic");
    }

    assert_eq!(
        mock.state().fetch_pr_status_snapshots_calls.len(),
        1,
        "the single-flight guard must collapse a cold stampede into one request"
    );
}

/// Unregistering the last PR releases the repository's cache so a stopped poller stops costing.
#[tokio::test]
async fn unregistering_the_last_pr_drops_the_repository_cache() {
    let hub = PrSnapshotHub::new();
    let (mock, github) = hub_github();
    hub.register(REPO, 101);
    hub.register(REPO, 102);
    hub.get_snapshot(REPO, 101, &github, Path::new(REPO))
        .await
        .expect("first read");

    hub.unregister(REPO, 102);
    assert_eq!(hub.registered_for_test(REPO), vec![101]);
    assert!(
        hub.snapshots.get(REPO).is_some(),
        "a still-watched repository keeps its cache"
    );

    hub.unregister(REPO, 101);
    assert!(hub.registered_for_test(REPO).is_empty());
    assert!(
        hub.snapshots.get(REPO).is_none(),
        "the last poller leaving must release the repository cache"
    );

    // A fresh registration after full release starts cold rather than serving old state.
    hub.register(REPO, 101);
    hub.get_snapshot(REPO, 101, &github, Path::new(REPO))
        .await
        .expect("read after re-registration");
    assert_eq!(mock.state().fetch_pr_status_snapshots_calls.len(), 2);
}

/// A PR read before its poller registers must still resolve — a brand-new PR is not invisible.
#[tokio::test]
async fn an_unregistered_pr_is_still_included_in_its_own_batch() {
    let hub = PrSnapshotHub::new();
    let (mock, github) = hub_github();

    let snapshot = hub
        .get_snapshot(REPO, 555, &github, Path::new(REPO))
        .await
        .expect("an unregistered PR must still resolve");

    assert_eq!(
        snapshot.sync_state.head_ref_oid.as_deref(),
        Some("batched-head-555")
    );
    assert_eq!(
        mock.state().fetch_pr_status_snapshots_calls[0],
        vec![555],
        "the requested PR must be added to the batch it triggered"
    );
}

/// `PrHealth` built from a snapshot must be identical to one read directly, except that comments
/// come from their own cached path.
#[test]
fn health_rebuilt_from_a_snapshot_preserves_every_field() {
    let checks = vec![PrHealthCheck {
        name: "build".to_string(),
        status: Some("COMPLETED".to_string()),
        conclusion: Some("SUCCESS".to_string()),
        details_url: Some("https://example.test/run".to_string()),
    }];
    let snapshot = PrStatusSnapshot {
        sync_state: PrSyncState {
            status: PrStatus::Open,
            merge_state_status: None,
            mergeable: None,
            is_draft: false,
            head_ref_name: "feature".to_string(),
            base_ref_name: "main".to_string(),
            head_ref_oid: Some("abc123".to_string()),
            base_ref_oid: Some("base-oid".to_string()),
        },
        review_decision: Some("APPROVED".to_string()),
        checks: checks.clone(),
        auto_merge_request: None,
    };

    let health = PrHealth::from_snapshot_and_comments(snapshot.clone(), Vec::new());

    assert_eq!(health.sync_state, snapshot.sync_state);
    assert_eq!(health.review_decision, snapshot.review_decision);
    assert_eq!(health.checks, checks);
    assert_eq!(health.auto_merge_request, snapshot.auto_merge_request);
    assert!(health.issue_comments.is_empty());
}
