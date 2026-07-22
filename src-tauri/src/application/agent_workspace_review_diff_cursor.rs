use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::application::agent_workspace_review_diff::{
    AgentWorkspaceReviewDiffSource, ReviewDiffSnapshot,
};
use crate::error::{AppError, AppResult};

const REVIEW_CURSOR_MAX_CHARS: usize = 8_192;
const REVIEW_CURSOR_MAX_BYTES: usize = 4_096;
pub(crate) const REVIEW_CURSOR_MAX_OFFSET: usize = 10_000_000;
const REVIEW_CURSOR_MAX_PATH_CHARS: usize = 512;
pub(crate) const REVIEW_FINGERPRINT_CHARS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewDiffCursorKind {
    Files,
    Diff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReviewDiffCursor {
    pub(crate) version: u8,
    pub(crate) kind: ReviewDiffCursorKind,
    pub(crate) target_scope: String,
    pub(crate) target_fingerprint: String,
    pub(crate) source_fingerprint: String,
    pub(crate) offset: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<AgentWorkspaceReviewDiffSource>,
}

pub(crate) fn bounded_limit(
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

pub(crate) fn validate_path_bound(path: &str) -> AppResult<()> {
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

pub(crate) fn encode_cursor(cursor: &ReviewDiffCursor) -> AppResult<String> {
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

pub(crate) fn decode_cursor(
    value: &str,
    expected_kind: ReviewDiffCursorKind,
) -> AppResult<ReviewDiffCursor> {
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

pub(crate) fn validate_cursor_snapshot(
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
