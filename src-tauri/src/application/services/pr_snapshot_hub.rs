//! Shared per-repository PR snapshot cache for the agent workspace pollers.
//!
//! Every monitored workspace used to read its own PR independently, so N workspaces watching one
//! repository cost N GitHub reads per poll tick. The 2026-08-11 rate-limit incident had 16 such
//! workspaces running concurrently.
//!
//! GitHub's GraphQL primary limit is point-based, and points scale with requested nodes rather
//! than with request count — so batching only helps if the node set stays small. Measured against
//! this repository before the hub was wired in: one aliased query covering 16 PRs reported
//! `rateLimit.cost = 1`, versus cost 1 apiece for 16 separate reads. That 16× reduction is what
//! justifies the subsystem; the request-count and latency wins are secondary.
//!
//! The batched query requests up to `PR_SNAPSHOT_CHECK_CONTEXT_LIMIT` (100) status-check
//! contexts per PR. When a PR's rollup exceeds that limit the parser omits it from the batch
//! map and `get_snapshot` falls back to the uncapped per-PR `fetch_pr_health` call automatically.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;

use crate::domain::services::github_service::{GithubServiceTrait, PrStatusSnapshot};
use crate::infrastructure::agents::claude::git_runtime_config;
use crate::AppResult;

/// One repository's most recent batched read.
struct RepoSnapshot {
    fetched_at: Instant,
    by_pr: HashMap<i64, PrStatusSnapshot>,
}

/// Caches batched PR reads per repository and serves them to every poller watching that repo.
///
/// Keyed by the project's working directory. A project with several remotes would share one key;
/// RalphX projects are single-origin today, and a shared key would only cause a redundant per-PR
/// fallback rather than a wrong answer.
#[derive(Default)]
pub struct PrSnapshotHub {
    /// PRs each repository currently has a live poller for.
    registrations: DashMap<String, HashSet<i64>>,
    /// Most recent batched result per repository.
    snapshots: DashMap<String, RepoSnapshot>,
    /// Single-flight guard per repository, so a stale window triggers one refresh, not N.
    refresh_locks: DashMap<String, Arc<tokio::sync::Mutex<()>>>,
}

impl PrSnapshotHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn ttl() -> Duration {
        Duration::from_secs(git_runtime_config().pr_snapshot_hub_ttl_secs.max(1))
    }

    /// Announces that a poller is now watching `pr_number` in `repo_key`.
    pub fn register(&self, repo_key: &str, pr_number: i64) {
        self.registrations
            .entry(repo_key.to_string())
            .or_default()
            .insert(pr_number);
    }

    /// Withdraws a PR when its poller exits, so later batches stop paying for it.
    pub fn unregister(&self, repo_key: &str, pr_number: i64) {
        let now_empty = match self.registrations.get_mut(repo_key) {
            Some(mut prs) => {
                prs.remove(&pr_number);
                prs.is_empty()
            }
            None => false,
        };
        if now_empty {
            self.registrations.remove(repo_key);
            self.snapshots.remove(repo_key);
            self.refresh_locks.remove(repo_key);
        }
    }

    #[cfg(test)]
    pub fn registered_for_test(&self, repo_key: &str) -> Vec<i64> {
        let mut prs: Vec<i64> = self
            .registrations
            .get(repo_key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default();
        prs.sort_unstable();
        prs
    }

    /// Reads one PR, serving the shared batch when it is fresh.
    ///
    /// A stale window triggers exactly one batched refresh covering every registered PR; other
    /// callers wait on the same lock and then read the result. A PR the batch did not return —
    /// either because it is brand-new (registered after the batch was built) or because its
    /// status-check rollup exceeded the batched context limit — falls back to its own read, which
    /// has no context cap.
    pub async fn get_snapshot(
        &self,
        repo_key: &str,
        pr_number: i64,
        github: &Arc<dyn GithubServiceTrait>,
        working_dir: &Path,
    ) -> AppResult<PrStatusSnapshot> {
        let ttl = Self::ttl();
        if let Some(snapshot) = self.fresh_snapshot(repo_key, pr_number, ttl) {
            return Ok(snapshot);
        }

        let lock = self
            .refresh_locks
            .entry(repo_key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // Another caller may have refreshed while this one waited for the lock.
        if let Some(snapshot) = self.fresh_snapshot(repo_key, pr_number, ttl) {
            return Ok(snapshot);
        }

        let mut pr_numbers: Vec<i64> = self
            .registrations
            .get(repo_key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default();
        if !pr_numbers.contains(&pr_number) {
            pr_numbers.push(pr_number);
        }
        pr_numbers.sort_unstable();

        let by_pr = github
            .fetch_pr_status_snapshots(working_dir, &pr_numbers)
            .await?;
        let found = by_pr.get(&pr_number).cloned();
        self.snapshots.insert(
            repo_key.to_string(),
            RepoSnapshot {
                fetched_at: Instant::now(),
                by_pr,
            },
        );

        match found {
            Some(snapshot) => Ok(snapshot),
            None => {
                let health = github.fetch_pr_health(working_dir, pr_number).await?;
                Ok(PrStatusSnapshot {
                    sync_state: health.sync_state,
                    review_decision: health.review_decision,
                    checks: health.checks,
                    auto_merge_request: health.auto_merge_request,
                })
            }
        }
    }

    /// Returns a cached snapshot only while it is inside the TTL.
    ///
    /// Deliberately never widens to `TTL × 2` or any other grace window: PR supervision acts on
    /// this data, so serving a knowingly expired snapshot would let a fixer or terminalization
    /// decision run on state GitHub has already moved past.
    fn fresh_snapshot(
        &self,
        repo_key: &str,
        pr_number: i64,
        ttl: Duration,
    ) -> Option<PrStatusSnapshot> {
        let entry = self.snapshots.get(repo_key)?;
        if entry.fetched_at.elapsed() > ttl {
            return None;
        }
        entry.by_pr.get(&pr_number).cloned()
    }
}

#[cfg(test)]
#[path = "pr_snapshot_hub_tests.rs"]
mod tests;
