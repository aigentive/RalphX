use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;

use crate::application::chat_service::escape_attr;
use crate::application::git_service::git_cmd;
use crate::domain::services::PrSyncState;
use crate::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DirtyCounts {
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BranchStatusSnapshot {
    pub dirty_known: bool,
    pub staged: usize,
    pub unstaged: usize,
    pub untracked: usize,
    pub dirty_as_of: DateTime<Utc>,
    pub behind_base: Option<u32>,
    pub ahead_of_base: Option<u32>,
    pub base_ref: Option<String>,
    pub base_relation: Option<String>,
    pub base_as_of: Option<DateTime<Utc>>,
}

#[derive(Clone, Default)]
pub(crate) struct BranchStatusCache {
    entries: Arc<DashMap<PathBuf, BranchStatusSnapshot>>,
    refreshes_in_flight: Arc<DashMap<PathBuf, ()>>,
}

impl BranchStatusCache {
    pub(crate) fn snapshot(&self, workspace_path: &Path) -> Option<BranchStatusSnapshot> {
        self.entries.get(workspace_path).map(|entry| entry.clone())
    }

    pub(crate) fn record(&self, workspace_path: &Path, snapshot: BranchStatusSnapshot) {
        self.entries.insert(workspace_path.to_path_buf(), snapshot);
    }

    pub(crate) fn refresh_due(
        &self,
        workspace_path: &Path,
        now: DateTime<Utc>,
        refresh_after: Duration,
    ) -> bool {
        self.snapshot(workspace_path)
            .map(|snapshot| {
                !snapshot.dirty_known
                    || now.signed_duration_since(snapshot.dirty_as_of) >= refresh_after
            })
            .unwrap_or(true)
    }

    pub(crate) fn claim_refresh(&self, workspace_path: &Path) -> bool {
        self.refreshes_in_flight
            .insert(workspace_path.to_path_buf(), ())
            .is_none()
    }

    pub(crate) fn finish_refresh(&self, workspace_path: &Path) {
        self.refreshes_in_flight.remove(workspace_path);
    }

    pub(crate) fn schedule_refresh_if_due(
        &self,
        workspace_path: PathBuf,
        base_ref: Option<String>,
        refresh_after: Duration,
    ) {
        if !self.refresh_due(&workspace_path, Utc::now(), refresh_after)
            || !self.claim_refresh(&workspace_path)
        {
            return;
        }
        let cache = self.clone();
        tokio::spawn(async move {
            let refresh_result = cache
                .refresh_local(&workspace_path, base_ref.as_deref())
                .await;
            cache.finish_refresh(&workspace_path);
            if let Err(error) = refresh_result {
                tracing::warn!(
                    workspace_path = %workspace_path.display(),
                    error = %error,
                    "scheduled runtime branch-status refresh failed"
                );
            }
        });
    }

    pub(crate) fn observe_pr_sync(
        &self,
        workspace_path: &Path,
        sync_state: &PrSyncState,
        observed_at: DateTime<Utc>,
    ) {
        let mut snapshot = self
            .snapshot(workspace_path)
            .unwrap_or(BranchStatusSnapshot {
                dirty_known: false,
                staged: 0,
                unstaged: 0,
                untracked: 0,
                dirty_as_of: observed_at,
                behind_base: None,
                ahead_of_base: None,
                base_ref: None,
                base_relation: None,
                base_as_of: None,
            });
        if snapshot
            .base_as_of
            .is_some_and(|current| current > observed_at)
        {
            return;
        }
        snapshot.base_ref = Some(sync_state.base_ref_name.clone());
        snapshot.base_relation = sync_state.merge_state_status.as_ref().map(|status| {
            use crate::domain::services::PrMergeStateStatus;
            match status {
                PrMergeStateStatus::Behind => "behind",
                PrMergeStateStatus::Dirty => "conflicting",
                PrMergeStateStatus::Clean => "mergeable",
                PrMergeStateStatus::Blocked => "blocked",
                PrMergeStateStatus::Draft => "draft",
                PrMergeStateStatus::Unknown => "unknown",
                PrMergeStateStatus::Unstable => "unstable",
                PrMergeStateStatus::HasHooks => "hooks",
                PrMergeStateStatus::Other(_) => "other",
            }
            .to_string()
        });
        snapshot.behind_base = None;
        snapshot.ahead_of_base = None;
        snapshot.base_as_of = Some(observed_at);
        self.record(workspace_path, snapshot);
    }

    pub(crate) async fn refresh_local(
        &self,
        workspace_path: &Path,
        base_ref: Option<&str>,
    ) -> AppResult<()> {
        let observed_at = Utc::now();
        let status = git_cmd::run(&["status", "--porcelain=v1", "-uall"], workspace_path).await?;
        if !status.status.success() {
            return Err(AppError::GitOperation(format!(
                "Failed to refresh runtime branch status: {}",
                String::from_utf8_lossy(&status.stderr).trim()
            )));
        }
        let counts = parse_porcelain_counts(&String::from_utf8_lossy(&status.stdout));
        let (mut behind_base, mut ahead_of_base, mut base_relation, mut base_as_of) =
            match base_ref.filter(|value| !value.trim().is_empty()) {
                Some(base_ref) => match local_base_counts(workspace_path, base_ref).await {
                    Ok((behind, ahead)) => (
                        Some(behind),
                        Some(ahead),
                        Some(relation_for_counts(behind, ahead).to_string()),
                        Some(observed_at),
                    ),
                    Err(error) => {
                        tracing::warn!(
                            workspace_path = %workspace_path.display(),
                            base_ref,
                            error = %error,
                            "local base comparison unavailable for runtime branch status"
                        );
                        (None, None, Some("unknown".to_string()), Some(observed_at))
                    }
                },
                None => (None, None, None, None),
            };
        let mut resolved_base_ref = base_ref.map(str::to_string);
        if let Some(existing) = self.snapshot(workspace_path) {
            let existing_is_newer = match (existing.base_as_of, base_as_of) {
                (Some(existing_at), Some(candidate_at)) => existing_at > candidate_at,
                (Some(_), None) => true,
                _ => false,
            };
            if existing_is_newer {
                behind_base = existing.behind_base;
                ahead_of_base = existing.ahead_of_base;
                resolved_base_ref = existing.base_ref;
                base_relation = existing.base_relation;
                base_as_of = existing.base_as_of;
            }
        }
        self.record(
            workspace_path,
            BranchStatusSnapshot {
                dirty_known: true,
                staged: counts.staged,
                unstaged: counts.unstaged,
                untracked: counts.untracked,
                dirty_as_of: observed_at,
                behind_base,
                ahead_of_base,
                base_ref: resolved_base_ref,
                base_relation,
                base_as_of,
            },
        );
        Ok(())
    }
}

async fn local_base_counts(workspace_path: &Path, base_ref: &str) -> AppResult<(u32, u32)> {
    let range = format!("{base_ref}...HEAD");
    let output = git_cmd::run(
        &["rev-list", "--left-right", "--count", &range],
        workspace_path,
    )
    .await?;
    if !output.status.success() {
        return Err(AppError::GitOperation(format!(
            "Failed to compare local base ref '{}': {}",
            base_ref,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut counts = text.split_whitespace();
    let behind = counts
        .next()
        .ok_or_else(|| AppError::GitOperation("Missing behind count".to_string()))?
        .parse::<u32>()
        .map_err(|error| AppError::GitOperation(format!("Invalid behind count: {error}")))?;
    let ahead = counts
        .next()
        .ok_or_else(|| AppError::GitOperation("Missing ahead count".to_string()))?
        .parse::<u32>()
        .map_err(|error| AppError::GitOperation(format!("Invalid ahead count: {error}")))?;
    Ok((behind, ahead))
}

fn relation_for_counts(behind: u32, ahead: u32) -> &'static str {
    match (behind, ahead) {
        (0, 0) => "even",
        (0, _) => "ahead",
        (_, 0) => "behind",
        _ => "diverged",
    }
}

pub(crate) fn parse_porcelain_counts(output: &str) -> DirtyCounts {
    output
        .lines()
        .fold(DirtyCounts::default(), |mut counts, line| {
            let status = line.as_bytes();
            if status.starts_with(b"??") {
                counts.untracked += 1;
            } else if status.len() >= 2 {
                if status[0] != b' ' {
                    counts.staged += 1;
                }
                if status[1] != b' ' {
                    counts.unstaged += 1;
                }
            }
            counts
        })
}

pub(crate) fn render_branch_status(
    cache: &BranchStatusCache,
    workspace_path: &Path,
    now: DateTime<Utc>,
    stale_after: Duration,
) -> String {
    let Some(snapshot) = cache.snapshot(workspace_path) else {
        return "<branch_status>\n<dirty state=\"unknown\"/>\n<base state=\"unknown\"/>\n</branch_status>"
            .to_string();
    };
    let mut block = "<branch_status>\n".to_string();
    if snapshot.dirty_known {
        let dirty_age = now
            .signed_duration_since(snapshot.dirty_as_of)
            .num_seconds()
            .max(0);
        let dirty_stale = dirty_age > stale_after.num_seconds();
        block.push_str(&format!(
            "<dirty state=\"known\" staged=\"{}\" unstaged=\"{}\" untracked=\"{}\" as_of=\"{}\"",
            snapshot.staged,
            snapshot.unstaged,
            snapshot.untracked,
            snapshot.dirty_as_of.to_rfc3339(),
        ));
        if dirty_stale {
            block.push_str(&format!(" stale=\"true\" age_seconds=\"{dirty_age}\""));
        }
        block.push_str("/>\n");
    } else {
        block.push_str("<dirty state=\"unknown\"/>\n");
    }
    match snapshot.base_as_of {
        Some(base_as_of) => {
            let base_age = now.signed_duration_since(base_as_of).num_seconds().max(0);
            block.push_str(&format!(
                "<base state=\"known\" base_ref=\"{}\" relation=\"{}\" as_of=\"{}\"",
                escape_attr(snapshot.base_ref.as_deref().unwrap_or_default()),
                escape_attr(snapshot.base_relation.as_deref().unwrap_or("unknown")),
                base_as_of.to_rfc3339(),
            ));
            if let Some(behind) = snapshot.behind_base {
                block.push_str(&format!(" behind=\"{behind}\""));
            }
            if let Some(ahead) = snapshot.ahead_of_base {
                block.push_str(&format!(" ahead=\"{ahead}\""));
            }
            if base_age > stale_after.num_seconds() {
                block.push_str(&format!(" stale=\"true\" age_seconds=\"{base_age}\""));
            }
            block.push_str("/>\n");
        }
        None => block.push_str("<base state=\"unknown\"/>\n"),
    }
    block.push_str("</branch_status>");
    block
}
