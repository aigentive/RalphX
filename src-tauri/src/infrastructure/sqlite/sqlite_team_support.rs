//! Shared row-codec helpers for the managed Team SQLite repositories.
//!
//! All decoders fail closed: malformed stored values surface as
//! `AppError::Database` instead of silently collapsing into defaults.

use chrono::{DateTime, Utc};

use crate::error::{AppError, AppResult};

pub(crate) fn parse_team_timestamp(value: &str, label: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| AppError::Database(format!("invalid {label} timestamp: {error}")))
}

pub(crate) fn parse_opt_team_timestamp(
    value: Option<String>,
    label: &str,
) -> AppResult<Option<DateTime<Utc>>> {
    value
        .map(|raw| parse_team_timestamp(&raw, label))
        .transpose()
}

/// Serializes a snake_case serde enum to its bare database string (no quotes).
pub(crate) fn enum_to_db<T: serde::Serialize>(value: &T, label: &str) -> AppResult<String> {
    let raw = serde_json::to_string(value)
        .map_err(|error| AppError::Database(format!("failed to encode {label}: {error}")))?;
    Ok(raw.trim_matches('"').to_string())
}

/// Decodes a bare database string back into a snake_case serde enum.
pub(crate) fn enum_from_db<T: serde::de::DeserializeOwned>(
    value: String,
    label: &str,
) -> AppResult<T> {
    serde_json::from_value(serde_json::Value::String(value))
        .map_err(|error| AppError::Database(format!("invalid {label}: {error}")))
}
