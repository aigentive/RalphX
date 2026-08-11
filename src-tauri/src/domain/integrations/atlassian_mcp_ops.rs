//! Request and response shapes for the Atlassian MCP tool operations.
//!
//! These are the write-side and long-tail operations that the shipped
//! integration surface never needed: Jira issue create/update, Confluence page
//! read/create/update, and a bounded generic API proxy.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Allowed relative-path prefixes for [`raw_api_request`](super::atlassian_resources::AtlassianApiClient::raw_api_request).
pub const ATLASSIAN_RAW_PATH_PREFIXES: &[&str] = &[
    "/rest/api/",
    "/rest/agile/",
    "/wiki/rest/api/",
    "/wiki/api/v2/",
];

/// Maximum serialized JSON size accepted from a raw Atlassian API response.
pub const ATLASSIAN_RAW_RESPONSE_MAX_BYTES: usize = 256 * 1024;

/// HTTP methods the generic Atlassian proxy accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AtlassianRawMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

impl AtlassianRawMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Head => "HEAD",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }

    /// Whether this method only reads. Read-tier callers are limited to these.
    pub const fn is_read_only(self) -> bool {
        matches!(self, Self::Get | Self::Head)
    }
}

impl std::str::FromStr for AtlassianRawMethod {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_uppercase().as_str() {
            "GET" => Ok(Self::Get),
            "HEAD" => Ok(Self::Head),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "PATCH" => Ok(Self::Patch),
            "DELETE" => Ok(Self::Delete),
            other => Err(format!("Unsupported Atlassian HTTP method: {other}")),
        }
    }
}

/// Fields accepted when creating a Jira issue.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JiraIssueCreateRequest {
    pub project_key: String,
    pub issue_type: String,
    pub summary: String,
    /// Plain text; wrapped into a minimal ADF paragraph for Jira Cloud v3.
    pub description: Option<String>,
    pub labels: Vec<String>,
    pub priority: Option<String>,
}

/// Fields accepted when updating a Jira issue. `None` leaves the field alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JiraIssueUpdateRequest {
    pub summary: Option<String>,
    /// Plain text; wrapped into a minimal ADF paragraph for Jira Cloud v3.
    pub description: Option<String>,
    pub labels: Option<Vec<String>>,
    pub priority: Option<String>,
}

impl JiraIssueUpdateRequest {
    pub fn is_empty(&self) -> bool {
        self.summary.is_none()
            && self.description.is_none()
            && self.labels.is_none()
            && self.priority.is_none()
    }
}

/// A newly created Jira issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JiraIssueCreated {
    pub id: String,
    pub key: String,
    pub url: String,
}

/// A Confluence page with its storage-format body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfluencePageContent {
    pub id: String,
    pub title: String,
    pub space_id: Option<String>,
    pub version: i64,
    pub body_storage: String,
    pub url: Option<String>,
}

/// Fields accepted when creating a Confluence page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfluencePageCreateRequest {
    pub space_id: String,
    pub title: String,
    /// Confluence storage-format body.
    pub body_storage: String,
    pub parent_id: Option<String>,
}

/// Fields accepted when updating a Confluence page. The current version is read
/// first and incremented, so callers never supply a version number.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfluencePageUpdateRequest {
    pub title: Option<String>,
    pub body_storage: Option<String>,
}

impl ConfluencePageUpdateRequest {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.body_storage.is_none()
    }
}

/// A raw Atlassian API response, already bounded in size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlassianRawResponse {
    pub body: Value,
}

/// Validate a caller-supplied relative Atlassian API path.
///
/// Unsafe input is rejected outright rather than sanitized: absolute URLs,
/// scheme/host injection, protocol-relative paths, `..` segments, and any path
/// outside [`ATLASSIAN_RAW_PATH_PREFIXES`].
///
/// # Errors
///
/// Returns a message describing the rejection reason.
pub fn validate_atlassian_raw_path(path: &str) -> Result<&str, String> {
    if path.is_empty() {
        return Err("Atlassian API path is required".to_string());
    }
    if path.contains("://") || path.contains('\\') {
        return Err("Atlassian API path must be relative, not an absolute URL".to_string());
    }
    if path.chars().any(char::is_control) {
        return Err("Atlassian API path contains control characters".to_string());
    }
    if !path.starts_with('/') || path.starts_with("//") {
        return Err("Atlassian API path must start with a single '/' path separator".to_string());
    }

    let route = path.split(['?', '#']).next().unwrap_or(path);
    if route.split('/').any(|segment| segment == "..") {
        return Err("Atlassian API path must not contain '..' segments".to_string());
    }
    if !ATLASSIAN_RAW_PATH_PREFIXES
        .iter()
        .any(|prefix| route.starts_with(prefix))
    {
        return Err(
            "Atlassian API path must start with /rest/api/, /rest/agile/, /wiki/rest/api/, or /wiki/api/v2/"
                .to_string(),
        );
    }
    Ok(path)
}
