//! Incremental re-review support: what changed since the last settled review.
//!
//! By round nine of a review-fix loop, eight rounds of the delta have already been cleared. Giving
//! the reviewer the prior Overview plus the files that moved since lets it re-verify the delta and
//! the prior dispositions instead of re-reading everything from zero.
//!
//! The delta is advisory. It never narrows what the reviewer *may* read, and every failure mode
//! here fails open to a full review — a reviewer that reads too much is slow, one that reads a
//! falsely small delta misses regressions.

use std::collections::BTreeMap;
use std::process::Command;

use crate::application::agent_workspace_review::AgentWorkspaceReviewTarget;
use crate::domain::entities::AgentWorkspacePreviousReviewSnapshot;
use crate::infrastructure::tool_paths::resolve_git_cli_path;

/// One file that moved between the previously reviewed head and the current one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentWorkspaceReviewPreviousDeltaFile {
    pub path: String,
    pub status: String,
}

/// Files changed since the previously reviewed head, plus whether that answer is trustworthy.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentWorkspaceReviewPreviousDelta {
    pub files: Vec<AgentWorkspaceReviewPreviousDeltaFile>,
    /// `false` when the previous head is unreachable — after a rebase or base update, for example.
    ///
    /// The reviewer must fall back to a full review whenever this is `false`; the file list is
    /// then only a partial view of the current inventory, never proof that little changed.
    pub complete: bool,
}

/// Computes the delta between the previously reviewed head and the current target.
///
/// Returns `None` when there is nothing to compare against. Every git failure yields
/// `complete: false` rather than an error, because a missing delta must degrade the review to a
/// full pass, not fail it.
pub fn previous_review_delta(
    target: &AgentWorkspaceReviewTarget,
    previous: &AgentWorkspacePreviousReviewSnapshot,
    current_inventory_statuses: &BTreeMap<String, String>,
) -> Option<AgentWorkspaceReviewPreviousDelta> {
    let previous_head = previous
        .reviewed_head_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    let Some(head_delta) = committed_delta_since(target, previous_head) else {
        // Unreachable previous head (rebase, base update, pruned commit). Fail open.
        return Some(AgentWorkspaceReviewPreviousDelta {
            files: inventory_as_delta_files(current_inventory_statuses),
            complete: false,
        });
    };

    // Merge the committed delta with the current uncommitted inventory: a file staged but never
    // committed does not appear in `prev_head..head` yet is unquestionably unreviewed.
    let mut merged = head_delta;
    for (path, status) in current_inventory_statuses {
        merged.entry(path.clone()).or_insert_with(|| status.clone());
    }
    Some(AgentWorkspaceReviewPreviousDelta {
        files: merged
            .into_iter()
            .map(|(path, status)| AgentWorkspaceReviewPreviousDeltaFile { path, status })
            .collect(),
        complete: true,
    })
}

fn inventory_as_delta_files(
    statuses: &BTreeMap<String, String>,
) -> Vec<AgentWorkspaceReviewPreviousDeltaFile> {
    statuses
        .iter()
        .map(|(path, status)| AgentWorkspaceReviewPreviousDeltaFile {
            path: path.clone(),
            status: status.clone(),
        })
        .collect()
}

/// `git diff --name-status prev_head..current_head`, or `None` when the range cannot be resolved.
fn committed_delta_since(
    target: &AgentWorkspaceReviewTarget,
    previous_head: &str,
) -> Option<BTreeMap<String, String>> {
    let current_head = target
        .head_sha
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("HEAD");
    if previous_head == current_head {
        return Some(BTreeMap::new());
    }
    let output = Command::new(resolve_git_cli_path())
        .current_dir(&target.working_directory)
        .args([
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            previous_head,
            current_head,
            "--",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_name_status_z(&output.stdout)
}

/// Parses `--name-status -z` output. Mirrors the inventory parser, but returns `None` instead of
/// erroring: a malformed range here means "cannot compute a delta", which is a fail-open signal.
fn parse_name_status_z(stdout: &[u8]) -> Option<BTreeMap<String, String>> {
    let fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut statuses = BTreeMap::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status_token = std::str::from_utf8(fields[index]).ok()?;
        index += 1;
        let status_code = status_token.chars().next().unwrap_or('M');
        if matches!(status_code, 'R' | 'C') {
            if index + 1 >= fields.len() {
                return None;
            }
            // Skip the rename source; the post-change path is what the inventory reports.
            index += 1;
        } else if index >= fields.len() {
            return None;
        }
        let path = std::str::from_utf8(fields[index]).ok()?;
        index += 1;
        let status = match status_code {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        };
        statuses.insert(path.to_string(), status.to_string());
    }
    Some(statuses)
}
