//! Granola note records and the outbound Granola API port.
//!
//! The HTTP client that implements the port lives in `infrastructure`; the
//! rate limiter and orchestration service stay in `application`.

use async_trait::async_trait;

/// Auth context for Granola REST calls.
///
/// Granola's public API uses an HTTP bearer API key sent in the `Authorization`
/// header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaAuthContext {
    pub api_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaNoteDetail {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub transcript: Option<Vec<GranolaTranscriptEntry>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaNoteSummary {
    pub id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub summary: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaNoteListPage {
    pub notes: Vec<GranolaNoteSummary>,
    pub has_more: bool,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GranolaTranscriptEntry {
    pub speaker: Option<String>,
    pub text: String,
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GranolaApiError {
    NotFound,
    RateLimited,
    ApiError(String),
}

/// Boundary to the Granola public API.
#[async_trait]
pub trait GranolaApiClient: Send + Sync {
    /// Low-cost credential check. The production client issues a minimal request
    /// (for example `GET /v1/notes?page_size=1`) and never fetches transcripts.
    async fn validate(&self, auth: &GranolaAuthContext) -> Result<(), String>;

    #[cfg(test)]
    fn is_unavailable_for_tests(&self) -> bool {
        false
    }

    async fn fetch_note_detail(
        &self,
        _auth: &GranolaAuthContext,
        _note_id: &str,
        _include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        Err(GranolaApiError::ApiError(
            "Granola note detail fetch is unavailable".to_string(),
        ))
    }

    async fn list_notes(
        &self,
        _auth: &GranolaAuthContext,
        _page_size: usize,
        _cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, GranolaApiError> {
        Err(GranolaApiError::ApiError(
            "Granola note listing is unavailable".to_string(),
        ))
    }
}

pub fn is_valid_granola_note_id(note_id: &str) -> bool {
    note_id.strip_prefix("not_").is_some_and(|suffix| {
        suffix.len() == 14 && suffix.chars().all(|ch| ch.is_ascii_alphanumeric())
    })
}
