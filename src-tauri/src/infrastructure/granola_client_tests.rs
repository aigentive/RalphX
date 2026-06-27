use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::application::{GranolaApiClient, GranolaApiError, GranolaAuthContext};

use super::granola_client::{
    granola_authorization_header, GranolaJsonRequester, GranolaRequestError, HyperGranolaApiClient,
};

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: Method,
    url: String,
    token: String,
}

#[derive(Default)]
struct FakeGranolaRequester {
    responses: Mutex<VecDeque<Result<Value, GranolaRequestError>>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl FakeGranolaRequester {
    fn new(responses: Vec<Result<Value, GranolaRequestError>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait]
impl GranolaJsonRequester for FakeGranolaRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
    ) -> Result<Value, GranolaRequestError> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest {
                method,
                url,
                token: token.to_string(),
            });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| {
                Err(GranolaRequestError::Transport(
                    "unexpected Granola request".to_string(),
                ))
            })
    }
}

fn auth() -> GranolaAuthContext {
    GranolaAuthContext {
        api_token: "grn_test_token".to_string(),
    }
}

#[test]
fn authorization_header_uses_bearer_token() {
    assert_eq!(
        granola_authorization_header("grn_abc123"),
        "Bearer grn_abc123"
    );
}

#[test]
fn constructing_client_does_not_panic_when_roots_are_unavailable() {
    let result = std::panic::catch_unwind(HyperGranolaApiClient::new);
    assert!(result.is_ok());
}

#[tokio::test]
async fn validate_uses_minimal_notes_request() {
    let fake = FakeGranolaRequester::new(vec![Ok(json!({ "notes": [] }))]);
    let client: &dyn GranolaApiClient = &fake;

    client.validate(&auth()).await.expect("validate token");

    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://public-api.granola.ai/v1/notes?page_size=1"
    );
    assert_eq!(requests[0].token, "grn_test_token");
}

#[tokio::test]
async fn fetch_note_detail_builds_transcript_request_and_maps_response() {
    let fake = FakeGranolaRequester::new(vec![Ok(json!({
        "id": "not_1234567890ABCD",
        "title": "Weekly planning",
        "web_url": "https://granola.ai/notes/not_1234567890ABCD",
        "summary": { "markdown": "Summary decisions" },
        "transcript": [
            {
                "speaker": "Alex",
                "text": "Transcript line",
                "start_ms": 1000,
                "end_ms": 2500
            }
        ]
    }))]);
    let client: &dyn GranolaApiClient = &fake;

    let note = client
        .fetch_note_detail(&auth(), "not_1234567890ABCD", true)
        .await
        .expect("fetch note detail");

    assert_eq!(note.id, "not_1234567890ABCD");
    assert_eq!(note.title.as_deref(), Some("Weekly planning"));
    assert_eq!(note.summary.as_deref(), Some("Summary decisions"));
    assert_eq!(
        note.transcript.as_ref().expect("transcript")[0].text,
        "Transcript line"
    );
    let requests = fake.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://public-api.granola.ai/v1/notes/not_1234567890ABCD?include=transcript"
    );
}

#[tokio::test]
async fn fetch_note_detail_preserves_nested_speaker_metadata() {
    let fake = FakeGranolaRequester::new(vec![Ok(json!({
        "id": "not_1234567890ABCD",
        "title": "Weekly planning",
        "transcript": [
            {
                "speaker": {
                    "source": "person",
                    "diarization_label": "SPEAKER_1"
                },
                "text": "Nested speaker line",
                "start_ms": 1000,
                "end_ms": 2500
            }
        ]
    }))]);
    let client: &dyn GranolaApiClient = &fake;

    let note = client
        .fetch_note_detail(&auth(), "not_1234567890ABCD", true)
        .await
        .expect("fetch note detail");

    let transcript = note.transcript.expect("transcript");
    assert_eq!(transcript[0].speaker.as_deref(), Some("SPEAKER_1 (person)"));
    assert_eq!(transcript[0].text, "Nested speaker line");
}

#[tokio::test]
async fn fetch_note_detail_maps_not_found_rate_limit_and_invalid_id_without_token_leaks() {
    let not_found = FakeGranolaRequester::new(vec![Err(GranolaRequestError::HttpStatus(404))]);
    let client: &dyn GranolaApiClient = &not_found;
    assert_eq!(
        client
            .fetch_note_detail(&auth(), "not_1234567890ABCD", false)
            .await
            .expect_err("not found"),
        GranolaApiError::NotFound
    );

    let rate_limited = FakeGranolaRequester::new(vec![Err(GranolaRequestError::HttpStatus(429))]);
    let client: &dyn GranolaApiClient = &rate_limited;
    assert_eq!(
        client
            .fetch_note_detail(&auth(), "not_1234567890ABCD", false)
            .await
            .expect_err("rate limited"),
        GranolaApiError::RateLimited
    );

    let invalid = FakeGranolaRequester::default();
    let client: &dyn GranolaApiClient = &invalid;
    let error = client
        .fetch_note_detail(&auth(), "bad-note-id", false)
        .await
        .expect_err("invalid id");
    assert!(matches!(error, GranolaApiError::ApiError(_)));
    if let GranolaApiError::ApiError(message) = error {
        assert!(!message.contains("grn_test_token"));
    }
    assert!(invalid.requests().is_empty());
}
