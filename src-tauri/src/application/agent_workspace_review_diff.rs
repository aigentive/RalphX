use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::agent_workspace_review::{
    resolve_review_target, workspace_review_source_snapshot_fingerprint,
    AgentWorkspaceReviewChangedFile, AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewTarget,
};
use crate::application::agent_workspace_review_diff_cursor::{
    bounded_limit, decode_cursor, encode_cursor, validate_cursor_snapshot, validate_path_bound,
    ReviewDiffCursor, ReviewDiffCursorKind,
};
use crate::application::agent_workspace_review_diff_inventory::full_changed_file_inventory;
use crate::application::diff_service::{
    validate_worktree_diff_file_containment, DiffLineKind, DiffService, FileDiff, FileDiffPage,
    MAX_DIFF_PAGE_LIMIT,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewTargetScope, Project,
};
use crate::error::{AppError, AppResult};

const REVIEW_FILE_PAGE_DEFAULT_LIMIT: usize = 100;
const REVIEW_FILE_PAGE_MAX_LIMIT: usize = 200;
const REVIEW_DIFF_PAGE_DEFAULT_LIMIT: usize = 200;
const REVIEW_DIFF_PAGE_MAX_SERIALIZED_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWorkspaceReviewDiffSource {
    SelectedSource,
    Committed,
    Staged,
    Unstaged,
}

impl AgentWorkspaceReviewDiffSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SelectedSource => "selected_source",
            Self::Committed => "committed",
            Self::Staged => "staged",
            Self::Unstaged => "unstaged",
        }
    }
}

impl FromStr for AgentWorkspaceReviewDiffSource {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim() {
            "selected_source" => Ok(Self::SelectedSource),
            "committed" => Ok(Self::Committed),
            "staged" => Ok(Self::Staged),
            "unstaged" => Ok(Self::Unstaged),
            _ => Err(AppError::Validation(format!(
                "Unsupported workspace Review diff source: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceReviewFilePage {
    pub files: Vec<AgentWorkspaceReviewChangedFile>,
    pub offset: usize,
    pub limit: usize,
    pub total_count: usize,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentWorkspaceReviewDiffPage {
    pub source: AgentWorkspaceReviewDiffSource,
    pub page: FileDiffPage,
    pub hunk_anchors: Vec<AgentWorkspaceReviewHunkAnchor>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ReviewDiffSnapshot {
    pub(super) target: AgentWorkspaceReviewTarget,
    pub(super) source_fingerprint: String,
    pub(super) files: Vec<AgentWorkspaceReviewChangedFile>,
}

pub async fn list_workspace_review_files(
    workspace: &AgentConversationWorkspace,
    project: &Project,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> AppResult<AgentWorkspaceReviewFilePage> {
    let limit = bounded_limit(
        limit,
        REVIEW_FILE_PAGE_DEFAULT_LIMIT,
        REVIEW_FILE_PAGE_MAX_LIMIT,
        "workspace Review file page",
    )?;
    let snapshot = resolve_snapshot(workspace, project).await?;
    let offset = match cursor {
        Some(cursor) => {
            let cursor = decode_cursor(cursor, ReviewDiffCursorKind::Files)?;
            validate_cursor_snapshot(&cursor, &snapshot)?;
            cursor.offset
        }
        None => 0,
    };
    if offset > snapshot.files.len() || (offset == snapshot.files.len() && offset > 0) {
        return Err(AppError::Validation(
            "Workspace Review file cursor offset is out of range".to_string(),
        ));
    }

    let files = snapshot
        .files
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    let consumed = offset.saturating_add(files.len());
    let next_cursor = if consumed < snapshot.files.len() {
        Some(encode_cursor(&ReviewDiffCursor {
            version: 1,
            kind: ReviewDiffCursorKind::Files,
            target_scope: snapshot.target.scope.to_string(),
            target_fingerprint: snapshot.target.diff_fingerprint.clone(),
            source_fingerprint: snapshot.source_fingerprint.clone(),
            offset: consumed,
            path: None,
            source: None,
        })?)
    } else {
        None
    };
    ensure_snapshot_unchanged(
        workspace,
        project,
        &snapshot.target.diff_fingerprint,
        &snapshot.source_fingerprint,
    )
    .await?;

    Ok(AgentWorkspaceReviewFilePage {
        files,
        offset,
        limit,
        total_count: snapshot.files.len(),
        next_cursor,
    })
}

pub async fn get_workspace_review_diff_page(
    workspace: &AgentConversationWorkspace,
    project: &Project,
    cursor: Option<&str>,
    path: Option<&str>,
    source: Option<&str>,
    limit: Option<usize>,
) -> AppResult<AgentWorkspaceReviewDiffPage> {
    let limit = bounded_limit(
        limit,
        REVIEW_DIFF_PAGE_DEFAULT_LIMIT,
        MAX_DIFF_PAGE_LIMIT,
        "workspace Review diff page",
    )?;
    let snapshot = resolve_snapshot(workspace, project).await?;
    let (path, source, offset) = match cursor {
        Some(cursor) => {
            if path.is_some() || source.is_some() {
                return Err(AppError::Validation(
                    "Workspace Review diff continuation accepts only cursor and optional limit"
                        .to_string(),
                ));
            }
            let cursor = decode_cursor(cursor, ReviewDiffCursorKind::Diff)?;
            validate_cursor_snapshot(&cursor, &snapshot)?;
            let path = cursor.path.ok_or_else(|| {
                AppError::Validation("Workspace Review diff cursor is missing path".to_string())
            })?;
            let source = cursor.source.ok_or_else(|| {
                AppError::Validation("Workspace Review diff cursor is missing source".to_string())
            })?;
            (path, source, cursor.offset)
        }
        None => {
            let path = path
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| {
                    AppError::Validation(
                        "Workspace Review diff first page requires path".to_string(),
                    )
                })?
                .to_string();
            let source = source
                .ok_or_else(|| {
                    AppError::Validation(
                        "Workspace Review diff first page requires source".to_string(),
                    )
                })?
                .parse()?;
            (path, source, 0)
        }
    };
    validate_path_bound(&path)?;
    validate_source_for_target(source, snapshot.target.scope)?;
    ensure_file_source_membership(&snapshot.files, &path, source)?;

    let diff = resolve_workspace_review_file_diff(&snapshot.target, &path, source)?;
    let hunk_anchors = hunk_anchors_for_page(&diff, source, offset, limit);
    let page = DiffService::page_file_diff(diff, offset, limit)?;
    if offset > page.total_rows || (offset == page.total_rows && offset > 0) {
        return Err(AppError::Validation(
            "Workspace Review diff cursor offset is out of range".to_string(),
        ));
    }
    let next_cursor = page
        .next_offset
        .map(|next_offset| {
            encode_cursor(&ReviewDiffCursor {
                version: 1,
                kind: ReviewDiffCursorKind::Diff,
                target_scope: snapshot.target.scope.to_string(),
                target_fingerprint: snapshot.target.diff_fingerprint.clone(),
                source_fingerprint: snapshot.source_fingerprint.clone(),
                offset: next_offset,
                path: Some(path.clone()),
                source: Some(source),
            })
        })
        .transpose()?;
    ensure_snapshot_unchanged(
        workspace,
        project,
        &snapshot.target.diff_fingerprint,
        &snapshot.source_fingerprint,
    )
    .await?;

    let response = AgentWorkspaceReviewDiffPage {
        source,
        page,
        hunk_anchors,
        next_cursor,
    };
    let response_size = serde_json::to_vec(&response)
        .map_err(|error| {
            AppError::Infrastructure(format!(
                "Failed to measure workspace Review diff response: {error}"
            ))
        })?
        .len();
    if response_size > REVIEW_DIFF_PAGE_MAX_SERIALIZED_BYTES {
        return Err(AppError::Validation(format!(
            "Workspace Review diff response size exceeds the {REVIEW_DIFF_PAGE_MAX_SERIALIZED_BYTES}-byte limit"
        )));
    }
    Ok(response)
}

pub fn resolve_workspace_review_file_diff(
    target: &AgentWorkspaceReviewTarget,
    path: &str,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<FileDiff> {
    validate_source_for_target(source, target.scope)?;
    validate_path_bound(path)?;
    let service = DiffService::new();
    let root = target.working_directory.to_str().ok_or_else(|| {
        AppError::Validation("Workspace Review path is not valid UTF-8".to_string())
    })?;
    if source == AgentWorkspaceReviewDiffSource::Unstaged {
        validate_worktree_diff_file_containment(root, path)?;
    }
    match source {
        AgentWorkspaceReviewDiffSource::SelectedSource => {
            service.get_file_diff_between_refs(path, root, &target.base_ref, &target.head_ref)
        }
        AgentWorkspaceReviewDiffSource::Committed => {
            service.get_file_diff_between_refs(path, root, &target.base_ref, "HEAD")
        }
        AgentWorkspaceReviewDiffSource::Staged => service.get_staged_file_diff(path, root),
        AgentWorkspaceReviewDiffSource::Unstaged => service.get_unstaged_file_diff(path, root),
    }
}

pub fn all_hunk_anchors_for_file(
    target: &AgentWorkspaceReviewTarget,
    path: &str,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<Vec<AgentWorkspaceReviewHunkAnchor>> {
    let diff = resolve_workspace_review_file_diff(target, path, source)?;
    Ok(diff
        .hunks
        .iter()
        .map(|hunk| AgentWorkspaceReviewHunkAnchor {
            path: path.to_string(),
            source: source.as_str().to_string(),
            hunk_header: hunk.header.clone(),
            old_start: hunk.old_start,
            old_lines: hunk.old_lines,
            new_start: hunk.new_start,
            new_lines: hunk.new_lines,
        })
        .collect())
}

/// Hashes one file's patch-vs-base.
///
/// The hash covers exactly what determines this file's hunk anchors: every hunk header plus every
/// line's kind and content. Two cycles that produce the same hash produce the same anchors, which
/// is what makes an annotation still valid without re-anchoring. Binary files hash their binary
/// marker, since they have no hunks to annotate anyway.
pub fn workspace_review_file_patch_hash(
    target: &AgentWorkspaceReviewTarget,
    path: &str,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<String> {
    let diff = resolve_workspace_review_file_diff(target, path, source)?;
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    hasher.update(b"\0");
    hasher.update(source.as_str().as_bytes());
    hasher.update(b"\0");
    if diff.is_binary {
        hasher.update(b"binary");
        return Ok(format!("{:x}", hasher.finalize()));
    }
    for hunk in &diff.hunks {
        hasher.update(hunk.header.as_bytes());
        hasher.update(b"\0");
        for line in &hunk.lines {
            hasher.update(match line.kind {
                DiffLineKind::Context => b"c".as_slice(),
                DiffLineKind::Addition => b"+".as_slice(),
                DiffLineKind::Deletion => b"-".as_slice(),
            });
            hasher.update(line.content.as_bytes());
            hasher.update(b"\0");
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Per-file patch hashes for the given `(path, source)` selections.
///
/// Files whose diff cannot be read are simply absent from the map, which fails carry-forward
/// closed for them: a stale annotation describing code that has since changed is worse than no
/// annotation.
pub fn workspace_review_file_patch_hashes(
    target: &AgentWorkspaceReviewTarget,
    selections: &BTreeSet<(String, String)>,
) -> BTreeMap<(String, String), String> {
    let mut hashes = BTreeMap::new();
    for (path, source) in selections {
        let Ok(parsed_source) = source.parse::<AgentWorkspaceReviewDiffSource>() else {
            continue;
        };
        if validate_path_bound(path).is_err()
            || validate_source_for_target(parsed_source, target.scope).is_err()
        {
            continue;
        }
        if let Ok(hash) = workspace_review_file_patch_hash(target, path, parsed_source) {
            hashes.insert((path.clone(), source.clone()), hash);
        }
    }
    hashes
}

pub async fn full_hunk_anchors_for_requests(
    workspace: &AgentConversationWorkspace,
    project: &Project,
    expected_target_fingerprint: &str,
    selections: &BTreeSet<(String, String)>,
) -> AppResult<(Vec<AgentWorkspaceReviewHunkAnchor>, String)> {
    let snapshot = resolve_snapshot(workspace, project).await?;
    if snapshot.target.diff_fingerprint != expected_target_fingerprint {
        return Err(AppError::Conflict(
            "Workspace Review target changed before hunk validation".to_string(),
        ));
    }

    let mut anchors = Vec::new();
    for (path, source) in selections {
        let Ok(source) = source.parse::<AgentWorkspaceReviewDiffSource>() else {
            continue;
        };
        if validate_path_bound(path).is_err()
            || validate_source_for_target(source, snapshot.target.scope).is_err()
            || ensure_file_source_membership(&snapshot.files, path, source).is_err()
        {
            continue;
        }
        anchors.extend(all_hunk_anchors_for_file(&snapshot.target, path, source)?);
    }
    ensure_snapshot_unchanged(
        workspace,
        project,
        &snapshot.target.diff_fingerprint,
        &snapshot.source_fingerprint,
    )
    .await?;
    Ok((anchors, snapshot.source_fingerprint))
}

pub async fn ensure_workspace_review_snapshot_current(
    workspace: &AgentConversationWorkspace,
    project: &Project,
    target_fingerprint: &str,
    source_fingerprint: &str,
) -> AppResult<()> {
    ensure_snapshot_unchanged(workspace, project, target_fingerprint, source_fingerprint).await
}

async fn resolve_snapshot(
    workspace: &AgentConversationWorkspace,
    project: &Project,
) -> AppResult<ReviewDiffSnapshot> {
    let target = resolve_review_target(workspace, project)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("Workspace Review target is no longer current".to_string())
        })?;
    let source_fingerprint = workspace_review_source_snapshot_fingerprint(&target).await?;
    let files = full_changed_file_inventory(&target)?;
    Ok(ReviewDiffSnapshot {
        target,
        source_fingerprint,
        files,
    })
}

async fn ensure_snapshot_unchanged(
    workspace: &AgentConversationWorkspace,
    project: &Project,
    target_fingerprint: &str,
    source_fingerprint: &str,
) -> AppResult<()> {
    let current = resolve_review_target(workspace, project)
        .await?
        .ok_or_else(|| {
            AppError::Conflict("Workspace Review target changed during diff read".to_string())
        })?;
    let current_source_fingerprint = workspace_review_source_snapshot_fingerprint(&current).await?;
    if current.diff_fingerprint != target_fingerprint
        || current_source_fingerprint != source_fingerprint
    {
        return Err(AppError::Conflict(
            "Workspace Review target or source snapshot changed during diff read".to_string(),
        ));
    }
    Ok(())
}

fn hunk_anchors_for_page(
    diff: &FileDiff,
    source: AgentWorkspaceReviewDiffSource,
    offset: usize,
    limit: usize,
) -> Vec<AgentWorkspaceReviewHunkAnchor> {
    let page_end = offset.saturating_add(limit);
    let mut row_offset = 0usize;
    let mut anchors = Vec::new();
    for hunk in &diff.hunks {
        let hunk_start = row_offset;
        let hunk_end = hunk_start.saturating_add(1 + hunk.lines.len());
        if hunk_start < page_end && hunk_end > offset {
            anchors.push(AgentWorkspaceReviewHunkAnchor {
                path: diff.file_path.clone(),
                source: source.as_str().to_string(),
                hunk_header: hunk.header.clone(),
                old_start: hunk.old_start,
                old_lines: hunk.old_lines,
                new_start: hunk.new_start,
                new_lines: hunk.new_lines,
            });
        }
        row_offset = hunk_end;
    }
    anchors
}

fn ensure_file_source_membership(
    files: &[AgentWorkspaceReviewChangedFile],
    path: &str,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<()> {
    let source = source.as_str();
    if files
        .iter()
        .any(|file| file.path == path && file.sources.iter().any(|value| value == source))
    {
        return Ok(());
    }
    Err(AppError::Validation(format!(
        "Path is not present in the current workspace Review {source} source: {path}"
    )))
}

pub(crate) fn validate_source_for_target(
    source: AgentWorkspaceReviewDiffSource,
    scope: AgentWorkspaceReviewTargetScope,
) -> AppResult<()> {
    let valid = matches!(
        (scope, source),
        (
            AgentWorkspaceReviewTargetScope::SelectedSource,
            AgentWorkspaceReviewDiffSource::SelectedSource
        ) | (
            AgentWorkspaceReviewTargetScope::WorkspaceDelta,
            AgentWorkspaceReviewDiffSource::Committed
                | AgentWorkspaceReviewDiffSource::Staged
                | AgentWorkspaceReviewDiffSource::Unstaged
        )
    );
    if valid {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "Source {} is invalid for workspace Review target scope {scope}",
            source.as_str()
        )))
    }
}
