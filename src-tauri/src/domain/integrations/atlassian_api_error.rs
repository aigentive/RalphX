//! Status-preserving error for Atlassian HTTP calls.
//!
//! The previous shape flattened the response status into a display string
//! (`"Atlassian returned HTTP 429"`), which forced downstream code to parse
//! message text to classify failures. Callers must classify on
//! [`AtlassianApiError::status`] instead (CLAUDE.md rule 5).

use std::fmt;

/// Maximum number of bytes of an Atlassian error body retained for diagnostics.
const MAX_BODY_EXCERPT_BYTES: usize = 512;

/// An Atlassian API call that did not produce a usable JSON response.
///
/// `status` is `Some` only when Atlassian actually answered with a non-success
/// HTTP status. Transport failures, timeouts, and response-parse failures carry
/// `None`, so an absent status never reads as a successful call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtlassianApiError {
    pub status: Option<u16>,
    pub message: String,
}

impl AtlassianApiError {
    /// Atlassian answered with a non-success HTTP status.
    ///
    /// `body` is truncated on a char boundary and is never expected to contain
    /// credentials: request auth lives in headers, which are not echoed here.
    pub fn from_status(status: u16, body: &str) -> Self {
        let excerpt = truncate_on_char_boundary(body.trim(), MAX_BODY_EXCERPT_BYTES);
        let message = if excerpt.is_empty() {
            format!("Atlassian returned HTTP {status}")
        } else {
            format!("Atlassian returned HTTP {status}: {excerpt}")
        };
        Self {
            status: Some(status),
            message,
        }
    }

    /// A failure with no Atlassian HTTP status: transport, timeout, or parse.
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            status: None,
            message: message.into(),
        }
    }

    /// Atlassian rate limiting. Classified by status code, never message text.
    pub fn is_rate_limited(&self) -> bool {
        self.status == Some(429)
    }

    /// The requested Jira issue, Confluence page, or endpoint does not exist.
    pub fn is_not_found(&self) -> bool {
        self.status == Some(404)
    }

    /// Credentials were rejected or lack the required scope.
    pub fn is_auth_failure(&self) -> bool {
        matches!(self.status, Some(401) | Some(403))
    }
}

impl fmt::Display for AtlassianApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AtlassianApiError {}

impl From<AtlassianApiError> for String {
    fn from(error: AtlassianApiError) -> Self {
        error.message
    }
}

impl From<String> for AtlassianApiError {
    fn from(message: String) -> Self {
        Self::transport(message)
    }
}

impl From<&str> for AtlassianApiError {
    fn from(message: &str) -> Self {
        Self::transport(message)
    }
}

fn truncate_on_char_boundary(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
