use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;
use std::str::FromStr;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::application::agent_workspace_review::{
    resolve_review_target, workspace_review_source_snapshot_fingerprint,
    AgentWorkspaceReviewChangedFile, AgentWorkspaceReviewHunkAnchor, AgentWorkspaceReviewTarget,
};
use crate::application::diff_service::{
    validate_worktree_diff_file_containment, DiffService, FileChange, FileChangeStatus, FileDiff,
    FileDiffPage, MAX_DIFF_PAGE_LIMIT,
};
use crate::domain::entities::{
    AgentConversationWorkspace, AgentWorkspaceReviewTargetScope, Project,
};
use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::resolve_git_cli_path;

const REVIEW_FILE_PAGE_DEFAULT_LIMIT: usize = 100;
const REVIEW_FILE_PAGE_MAX_LIMIT: usize = 200;
const REVIEW_DIFF_PAGE_DEFAULT_LIMIT: usize = 200;
const REVIEW_CURSOR_MAX_CHARS: usize = 8_192;
const REVIEW_CURSOR_MAX_BYTES: usize = 4_096;
const REVIEW_CURSOR_MAX_OFFSET: usize = 10_000_000;
const REVIEW_CURSOR_MAX_PATH_CHARS: usize = 512;
const REVIEW_FINGERPRINT_CHARS: usize = 64;

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
struct ReviewDiffSnapshot {
    target: AgentWorkspaceReviewTarget,
    source_fingerprint: String,
    files: Vec<AgentWorkspaceReviewChangedFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ReviewDiffCursorKind {
    Files,
    Diff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewDiffCursor {
    version: u8,
    kind: ReviewDiffCursorKind,
    target_scope: String,
    target_fingerprint: String,
    source_fingerprint: String,
    offset: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<AgentWorkspaceReviewDiffSource>,
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

    Ok(AgentWorkspaceReviewDiffPage {
        source,
        page,
        hunk_anchors,
        next_cursor,
    })
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

fn full_changed_file_inventory(
    target: &AgentWorkspaceReviewTarget,
) -> AppResult<Vec<AgentWorkspaceReviewChangedFile>> {
    let service = DiffService::new();
    let root = target.working_directory.to_str().ok_or_else(|| {
        AppError::Validation("Workspace Review path is not valid UTF-8".to_string())
    })?;
    let sources = match target.scope {
        AgentWorkspaceReviewTargetScope::SelectedSource => vec![(
            AgentWorkspaceReviewDiffSource::SelectedSource,
            service.get_file_changes_between_refs(root, &target.base_ref, &target.head_ref)?,
        )],
        AgentWorkspaceReviewTargetScope::WorkspaceDelta => vec![
            (
                AgentWorkspaceReviewDiffSource::Committed,
                service.get_file_changes_between_refs(root, &target.base_ref, "HEAD")?,
            ),
            (
                AgentWorkspaceReviewDiffSource::Staged,
                service.get_staged_file_changes(root)?,
            ),
            (
                AgentWorkspaceReviewDiffSource::Unstaged,
                service.get_unstaged_file_changes(root)?,
            ),
        ],
    };

    let mut files = BTreeMap::<String, (String, BTreeSet<String>)>::new();
    for (source, changes) in sources {
        let source_statuses = source_file_statuses(target, source)?;
        for change in changes {
            let source_status = source_statuses.get(&change.path).map(String::as_str);
            let is_untracked_addition = source == AgentWorkspaceReviewDiffSource::Unstaged
                && source_status.is_none()
                && matches!(change.status, FileChangeStatus::Added);
            if source_status.is_none() && !is_untracked_addition {
                continue;
            }
            merge_file_change(&mut files, change, source, source_status);
        }
    }
    Ok(files
        .into_iter()
        .map(
            |(path, (status, sources))| AgentWorkspaceReviewChangedFile {
                path,
                status,
                sources: sources.into_iter().collect(),
            },
        )
        .collect())
}

fn merge_file_change(
    files: &mut BTreeMap<String, (String, BTreeSet<String>)>,
    change: FileChange,
    source: AgentWorkspaceReviewDiffSource,
    source_status: Option<&str>,
) {
    let status = source_status.unwrap_or(match change.status {
        FileChangeStatus::Added => "added",
        FileChangeStatus::Modified => "modified",
        FileChangeStatus::Deleted => "deleted",
    });
    let entry = files
        .entry(change.path)
        .or_insert_with(|| (status.to_string(), BTreeSet::new()));
    if status_rank(status) > status_rank(&entry.0) {
        entry.0 = status.to_string();
    }
    entry.1.insert(source.as_str().to_string());
}

fn status_rank(status: &str) -> u8 {
    match status {
        "deleted" => 4,
        "added" => 3,
        "renamed" => 2,
        "modified" => 1,
        _ => 0,
    }
}

fn source_file_statuses(
    target: &AgentWorkspaceReviewTarget,
    source: AgentWorkspaceReviewDiffSource,
) -> AppResult<BTreeMap<String, String>> {
    let mut command = Command::new(resolve_git_cli_path());
    command.current_dir(&target.working_directory).arg("diff");
    match source {
        AgentWorkspaceReviewDiffSource::SelectedSource => {
            command.args([
                "--name-status",
                "-z",
                "--find-renames",
                &target.base_ref,
                &target.head_ref,
                "--",
            ]);
        }
        AgentWorkspaceReviewDiffSource::Committed => {
            command.args([
                "--name-status",
                "-z",
                "--find-renames",
                &target.base_ref,
                "HEAD",
                "--",
            ]);
        }
        AgentWorkspaceReviewDiffSource::Staged => {
            command.args(["--cached", "--name-status", "-z", "--find-renames", "--"]);
        }
        AgentWorkspaceReviewDiffSource::Unstaged => {
            command.args(["--name-status", "-z", "--find-renames", "--"]);
        }
    }
    let output = command.output().map_err(|error| {
        AppError::GitOperation(format!(
            "Failed to read Workspace Review file statuses: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(AppError::GitOperation(format!(
            "Failed to read Workspace Review file statuses: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    parse_name_status_z(&output.stdout)
}

fn parse_name_status_z(stdout: &[u8]) -> AppResult<BTreeMap<String, String>> {
    let fields = stdout
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut statuses = BTreeMap::new();
    let mut index = 0usize;
    while index < fields.len() {
        let status_token = std::str::from_utf8(fields[index]).map_err(|_| {
            AppError::Validation("Workspace Review git status is not valid UTF-8".to_string())
        })?;
        index += 1;
        let status_code = status_token.chars().next().unwrap_or('M');
        if matches!(status_code, 'R' | 'C') {
            if index + 1 >= fields.len() {
                return Err(AppError::GitOperation(
                    "Workspace Review git rename status was incomplete".to_string(),
                ));
            }
            index += 1;
        } else if index >= fields.len() {
            return Err(AppError::GitOperation(
                "Workspace Review git file status was incomplete".to_string(),
            ));
        }
        let path = std::str::from_utf8(fields[index]).map_err(|_| {
            AppError::Validation("Workspace Review file path is not valid UTF-8".to_string())
        })?;
        index += 1;
        let status = match status_code {
            'A' => "added",
            'D' => "deleted",
            'R' => "renamed",
            _ => "modified",
        };
        statuses.insert(path.to_string(), status.to_string());
    }
    Ok(statuses)
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

fn validate_source_for_target(
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

fn bounded_limit(
    requested: Option<usize>,
    default: usize,
    max: usize,
    label: &str,
) -> AppResult<usize> {
    let limit = requested.unwrap_or(default);
    if limit == 0 || limit > max {
        return Err(AppError::Validation(format!(
            "{label} limit must be between 1 and {max}"
        )));
    }
    Ok(limit)
}

fn validate_path_bound(path: &str) -> AppResult<()> {
    if path.chars().count() > REVIEW_CURSOR_MAX_PATH_CHARS {
        return Err(AppError::Validation(format!(
            "Workspace Review diff path is limited to {REVIEW_CURSOR_MAX_PATH_CHARS} characters"
        )));
    }
    if path.trim().is_empty() {
        return Err(AppError::Validation(
            "Workspace Review diff path must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn encode_cursor(cursor: &ReviewDiffCursor) -> AppResult<String> {
    let bytes = serde_json::to_vec(cursor).map_err(|error| {
        AppError::Validation(format!("Failed to encode workspace Review cursor: {error}"))
    })?;
    if bytes.len() > REVIEW_CURSOR_MAX_BYTES {
        return Err(AppError::Validation(
            "Workspace Review cursor payload is too large".to_string(),
        ));
    }
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor(value: &str, expected_kind: ReviewDiffCursorKind) -> AppResult<ReviewDiffCursor> {
    if value.is_empty() || value.chars().count() > REVIEW_CURSOR_MAX_CHARS {
        return Err(AppError::Validation(
            "Workspace Review cursor is empty or too large".to_string(),
        ));
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| AppError::Validation("Workspace Review cursor is malformed".to_string()))?;
    if bytes.len() > REVIEW_CURSOR_MAX_BYTES {
        return Err(AppError::Validation(
            "Workspace Review cursor payload is too large".to_string(),
        ));
    }
    let cursor: ReviewDiffCursor = serde_json::from_slice(&bytes).map_err(|_| {
        AppError::Validation("Workspace Review cursor payload is malformed".to_string())
    })?;
    if cursor.version != 1 || cursor.kind != expected_kind {
        return Err(AppError::Validation(
            "Workspace Review cursor version or kind is invalid".to_string(),
        ));
    }
    if cursor.offset > REVIEW_CURSOR_MAX_OFFSET
        || cursor.target_fingerprint.len() != REVIEW_FINGERPRINT_CHARS
        || cursor.source_fingerprint.len() != REVIEW_FINGERPRINT_CHARS
    {
        return Err(AppError::Validation(
            "Workspace Review cursor fields are out of bounds".to_string(),
        ));
    }
    match cursor.kind {
        ReviewDiffCursorKind::Files if cursor.path.is_some() || cursor.source.is_some() => {
            return Err(AppError::Validation(
                "Workspace Review file cursor contains diff selection fields".to_string(),
            ));
        }
        ReviewDiffCursorKind::Diff => {
            let path = cursor.path.as_deref().ok_or_else(|| {
                AppError::Validation("Workspace Review diff cursor is missing path".to_string())
            })?;
            validate_path_bound(path)?;
            if cursor.source.is_none() {
                return Err(AppError::Validation(
                    "Workspace Review diff cursor is missing source".to_string(),
                ));
            }
        }
        ReviewDiffCursorKind::Files => {}
    }
    Ok(cursor)
}

fn validate_cursor_snapshot(
    cursor: &ReviewDiffCursor,
    snapshot: &ReviewDiffSnapshot,
) -> AppResult<()> {
    if cursor.target_scope != snapshot.target.scope.to_string()
        || cursor.target_fingerprint != snapshot.target.diff_fingerprint
        || cursor.source_fingerprint != snapshot.source_fingerprint
    {
        return Err(AppError::Conflict(
            "Workspace Review cursor is stale for the current target or source snapshot"
                .to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "agent_workspace_review_diff_tests.rs"]
mod tests;
