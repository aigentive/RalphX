use async_trait::async_trait;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;
use tokio::time::Duration;
use tokio_util::bytes::Bytes;

use crate::domain::integrations::{
    is_valid_granola_note_id, GranolaApiClient, GranolaApiError, GranolaAuthContext,
    GranolaNoteDetail, GranolaNoteListPage, GranolaNoteSummary, GranolaTranscriptEntry,
};

const GRANOLA_API_BASE: &str = "https://public-api.granola.ai";

pub struct HyperGranolaApiClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    timeout: Duration,
}

impl HyperGranolaApiClient {
    pub fn new() -> Result<Self, String> {
        install_rustls_crypto_provider();
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|error| format!("native root certificates unavailable: {error}"))?
            .https_only()
            .enable_http1()
            .build();
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(https),
            timeout: Duration::from_secs(20),
        })
    }

    async fn send_json_request(
        &self,
        method: Method,
        url: String,
        token: &str,
    ) -> Result<Value, GranolaRequestError> {
        let uri = url.parse::<hyper::Uri>().map_err(|error| {
            GranolaRequestError::Transport(format!("Invalid Granola URL: {error}"))
        })?;
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("Accept", "application/json")
            .header("Authorization", granola_authorization_header(token))
            .body(Full::new(Bytes::new()))
            .map_err(|error| {
                GranolaRequestError::Transport(format!("Failed to build Granola request: {error}"))
            })?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| GranolaRequestError::Transport("Granola request timed out".to_string()))?
            .map_err(|error| {
                GranolaRequestError::Transport(format!("Granola request failed: {error}"))
            })?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| {
                GranolaRequestError::Transport(format!("Failed to read Granola response: {error}"))
            })?
            .to_bytes();
        if !status.is_success() {
            return Err(GranolaRequestError::HttpStatus(status.as_u16()));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            GranolaRequestError::InvalidJson(format!("Failed to parse Granola response: {error}"))
        })
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

pub(crate) fn granola_authorization_header(token: &str) -> String {
    format!("Bearer {token}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GranolaRequestError {
    HttpStatus(u16),
    Transport(String),
    InvalidJson(String),
}

impl std::fmt::Display for GranolaRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HttpStatus(status) => write!(formatter, "Granola returned HTTP {status}"),
            Self::Transport(message) | Self::InvalidJson(message) => formatter.write_str(message),
        }
    }
}

#[async_trait]
pub(crate) trait GranolaJsonRequester: Send + Sync {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
    ) -> Result<Value, GranolaRequestError>;
}

#[async_trait]
impl GranolaJsonRequester for HyperGranolaApiClient {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
    ) -> Result<Value, GranolaRequestError> {
        self.send_json_request(method, url, token).await
    }
}

#[async_trait]
impl<T> GranolaApiClient for T
where
    T: GranolaJsonRequester + Send + Sync,
{
    async fn validate(&self, auth: &GranolaAuthContext) -> Result<(), String> {
        self.request_json(
            Method::GET,
            format!("{GRANOLA_API_BASE}/v1/notes?page_size=1"),
            &auth.api_token,
        )
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    async fn fetch_note_detail(
        &self,
        auth: &GranolaAuthContext,
        note_id: &str,
        include_transcript: bool,
    ) -> Result<GranolaNoteDetail, GranolaApiError> {
        if !is_valid_granola_note_id(note_id) {
            return Err(GranolaApiError::ApiError(
                "Granola note id is invalid".to_string(),
            ));
        }
        let include_query = if include_transcript {
            "?include=transcript"
        } else {
            ""
        };
        let value = self
            .request_json(
                Method::GET,
                format!("{GRANOLA_API_BASE}/v1/notes/{note_id}{include_query}"),
                &auth.api_token,
            )
            .await
            .map_err(granola_api_error_from_request_error)?;
        granola_note_detail_from_value(&value, note_id)
            .map_err(|error| GranolaApiError::ApiError(error.to_string()))
    }

    async fn list_notes(
        &self,
        auth: &GranolaAuthContext,
        page_size: usize,
        cursor: Option<&str>,
    ) -> Result<GranolaNoteListPage, GranolaApiError> {
        let page_size = page_size.clamp(1, 30);
        let mut url = format!("{GRANOLA_API_BASE}/v1/notes?page_size={page_size}");
        if let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) {
            url.push_str("&cursor=");
            url.push_str(&encode_query_value(cursor));
        }
        let value = self
            .request_json(Method::GET, url, &auth.api_token)
            .await
            .map_err(granola_api_error_from_request_error)?;
        granola_note_list_page_from_value(&value)
            .map_err(|error| GranolaApiError::ApiError(error.to_string()))
    }
}

fn granola_api_error_from_request_error(error: GranolaRequestError) -> GranolaApiError {
    match error {
        GranolaRequestError::HttpStatus(404) => GranolaApiError::NotFound,
        GranolaRequestError::HttpStatus(429) => GranolaApiError::RateLimited,
        other => GranolaApiError::ApiError(other.to_string()),
    }
}

fn granola_note_detail_from_value(
    value: &Value,
    fallback_id: &str,
) -> Result<GranolaNoteDetail, &'static str> {
    if !value.is_object() {
        return Err("Granola note response was not an object");
    }
    Ok(GranolaNoteDetail {
        id: string_field(value, &["id"]).unwrap_or_else(|| fallback_id.to_string()),
        title: string_field(value, &["title"]),
        url: string_field(value, &["web_url", "url"]),
        summary: summary_from_value(value),
        transcript: transcript_from_value(value),
    })
}

fn granola_note_list_page_from_value(value: &Value) -> Result<GranolaNoteListPage, &'static str> {
    let notes = value
        .get("notes")
        .and_then(Value::as_array)
        .ok_or("Granola notes response did not include a notes array")?;
    Ok(GranolaNoteListPage {
        notes: notes
            .iter()
            .filter_map(granola_note_summary_from_value)
            .collect(),
        has_more: value
            .get("hasMore")
            .or_else(|| value.get("has_more"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
        cursor: string_field(value, &["cursor", "next_cursor", "nextCursor"]),
    })
}

fn granola_note_summary_from_value(value: &Value) -> Option<GranolaNoteSummary> {
    let id = string_field(value, &["id"])?;
    if !is_valid_granola_note_id(&id) {
        return None;
    }
    Some(GranolaNoteSummary {
        id,
        title: string_field(value, &["title"]),
        url: string_field(value, &["web_url", "url"]),
        summary: summary_from_value(value),
        created_at: string_field(value, &["created_at", "createdAt"]),
        updated_at: string_field(value, &["updated_at", "updatedAt"]),
    })
}

fn string_field(value: &Value, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn summary_from_value(value: &Value) -> Option<String> {
    string_field(value, &["summary_markdown", "summary_text", "text"]).or_else(|| {
        value.get("summary").and_then(|summary| {
            summary
                .as_str()
                .map(ToOwned::to_owned)
                .or_else(|| string_field(summary, &["markdown", "text", "plain_text", "content"]))
        })
    })
}

fn transcript_from_value(value: &Value) -> Option<Vec<GranolaTranscriptEntry>> {
    let entries = value.get("transcript")?.as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                let text = string_field(entry, &["text", "content"])?;
                Some(GranolaTranscriptEntry {
                    speaker: transcript_speaker_from_value(entry),
                    text,
                    start_ms: u64_field(entry, &["start_ms", "startMs", "start_time_ms"]),
                    end_ms: u64_field(entry, &["end_ms", "endMs", "end_time_ms"]),
                })
            })
            .collect(),
    )
}

fn transcript_speaker_from_value(entry: &Value) -> Option<String> {
    string_field(entry, &["speaker", "speaker_name"])
        .or_else(|| speaker_metadata_from_value(entry))
        .or_else(|| entry.get("speaker").and_then(speaker_metadata_from_value))
}

fn speaker_metadata_from_value(value: &Value) -> Option<String> {
    let label = string_field(
        value,
        &["diarization_label", "diarizationLabel", "label", "name"],
    );
    let source = string_field(value, &["source"]);
    match (label, source) {
        (Some(label), Some(source)) if label == source => Some(label),
        (Some(label), Some(source)) => Some(format!("{label} ({source})")),
        (Some(label), None) => Some(label),
        (None, Some(source)) => Some(source),
        (None, None) => None,
    }
}

fn u64_field(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(name).and_then(Value::as_u64))
}

fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}
