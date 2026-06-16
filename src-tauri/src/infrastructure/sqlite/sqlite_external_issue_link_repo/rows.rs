use std::str::FromStr;

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};

use crate::domain::integrations::{
    ExternalIssueLink, ExternalIssueLocalObject, ExternalIssueLocalObjectKind,
    ExternalIssueSyncRecord, ExternalIssueSyncStatus,
};
use crate::error::{AppError, AppResult};

fn parse_datetime(raw: String) -> AppResult<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(&raw) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S") {
        return Ok(Utc.from_utc_datetime(&ndt));
    }
    Err(AppError::Database(format!("invalid datetime: {raw}")))
}

fn parse_optional_datetime(raw: Option<String>) -> AppResult<Option<DateTime<Utc>>> {
    raw.map(parse_datetime).transpose()
}

pub(super) fn row_to_link(row: &rusqlite::Row<'_>) -> AppResult<ExternalIssueLink> {
    let local_kind = row
        .get::<_, String>("local_object_kind")
        .map_err(|error| AppError::Database(error.to_string()))
        .and_then(|value| {
            ExternalIssueLocalObjectKind::from_str(&value).map_err(AppError::Database)
        })?;
    Ok(ExternalIssueLink {
        id: row
            .get("id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        provider: row
            .get("provider")
            .map_err(|error| AppError::Database(error.to_string()))?,
        external_kind: row
            .get("external_kind")
            .map_err(|error| AppError::Database(error.to_string()))?,
        external_id: row
            .get("external_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        external_key: row
            .get("external_key")
            .map_err(|error| AppError::Database(error.to_string()))?,
        external_url: row
            .get("external_url")
            .map_err(|error| AppError::Database(error.to_string()))?,
        local_object: ExternalIssueLocalObject {
            kind: local_kind,
            id: row
                .get("local_object_id")
                .map_err(|error| AppError::Database(error.to_string()))?,
        },
        local_project_id: row
            .get("local_project_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        local_sha: row
            .get("local_sha")
            .map_err(|error| AppError::Database(error.to_string()))?,
        local_state: row
            .get("local_state")
            .map_err(|error| AppError::Database(error.to_string()))?,
        idempotency_key: row
            .get("idempotency_key")
            .map_err(|error| AppError::Database(error.to_string()))?,
        metadata_json: row
            .get("metadata_json")
            .map_err(|error| AppError::Database(error.to_string()))?,
        created_at: parse_datetime(
            row.get("created_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        updated_at: parse_datetime(
            row.get("updated_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
    })
}

pub(super) fn row_to_sync_record(row: &rusqlite::Row<'_>) -> AppResult<ExternalIssueSyncRecord> {
    let status = row
        .get::<_, String>("status")
        .map_err(|error| AppError::Database(error.to_string()))
        .and_then(|value| ExternalIssueSyncStatus::from_str(&value).map_err(AppError::Database))?;
    Ok(ExternalIssueSyncRecord {
        id: row
            .get("id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        link_id: row
            .get("link_id")
            .map_err(|error| AppError::Database(error.to_string()))?,
        sync_kind: row
            .get("sync_kind")
            .map_err(|error| AppError::Database(error.to_string()))?,
        idempotency_key: row
            .get("idempotency_key")
            .map_err(|error| AppError::Database(error.to_string()))?,
        local_sha: row
            .get("local_sha")
            .map_err(|error| AppError::Database(error.to_string()))?,
        local_state: row
            .get("local_state")
            .map_err(|error| AppError::Database(error.to_string()))?,
        external_version: row
            .get("external_version")
            .map_err(|error| AppError::Database(error.to_string()))?,
        status,
        last_attempt_at: parse_optional_datetime(
            row.get("last_attempt_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        completed_at: parse_optional_datetime(
            row.get("completed_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        error_message: row
            .get("error_message")
            .map_err(|error| AppError::Database(error.to_string()))?,
        metadata_json: row
            .get("metadata_json")
            .map_err(|error| AppError::Database(error.to_string()))?,
        created_at: parse_datetime(
            row.get("created_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
        updated_at: parse_datetime(
            row.get("updated_at")
                .map_err(|error| AppError::Database(error.to_string()))?,
        )?,
    })
}

pub(super) fn link_select_sql() -> &'static str {
    "SELECT id, provider, external_kind, external_id, external_key, external_url,
            local_object_kind, local_object_id, local_project_id, local_sha, local_state,
            idempotency_key, metadata_json, created_at, updated_at
       FROM external_issue_links"
}

pub(super) fn sync_select_sql() -> &'static str {
    "SELECT id, link_id, sync_kind, idempotency_key, local_sha, local_state,
            external_version, status, last_attempt_at, completed_at, error_message,
            metadata_json, created_at, updated_at
       FROM external_issue_sync_records"
}

pub(super) fn now_text() -> String {
    Utc::now().to_rfc3339()
}

pub(super) fn completed_at_for(status: ExternalIssueSyncStatus) -> Option<String> {
    match status {
        ExternalIssueSyncStatus::Succeeded
        | ExternalIssueSyncStatus::Failed
        | ExternalIssueSyncStatus::Skipped => Some(now_text()),
        ExternalIssueSyncStatus::Pending => None,
    }
}
