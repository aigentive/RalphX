use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::domain::integrations::{AtlassianApiError, AtlassianAuthContext, AtlassianCredential};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{AtlassianJsonRequester, RequestAuth};
use super::confluence_secondary::{
    confluence_link_content, fetch_confluence_attachments, fetch_confluence_child_pages,
    fetch_confluence_footer_comments,
};

#[derive(Default)]
struct FakeRequester {
    responses: Mutex<VecDeque<Result<Value, AtlassianApiError>>>,
    requests: Mutex<Vec<String>>,
}

impl FakeRequester {
    fn new(responses: Vec<Result<Value, AtlassianApiError>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait]
impl AtlassianJsonRequester for FakeRequester {
    async fn request_json(
        &self,
        _method: Method,
        url: String,
        _auth: RequestAuth<'_>,
        _body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        self.requests.lock().expect("requests").push(url);
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err(AtlassianApiError::transport("unexpected request")))
    }
}

fn auth_context() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::ApiToken {
            email: "dev@example.com".to_string(),
            token: "token".to_string(),
        },
    }
}

fn integration_reference(kind: &str, id: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        key: None,
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

#[tokio::test]
async fn footer_comments_strip_storage_html_and_capture_author_id() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "results": [
            {
                "id": "111",
                "body": { "storage": { "value": "<p>Nice&nbsp;work <strong>team</strong></p>" } },
                "version": { "authorId": "712020:abc", "createdAt": "2024-01-01T00:00:00.000Z" }
            },
            {
                "id": "112",
                "body": { "storage": { "value": "<p>Second comment</p>" } },
                "version": { "authorId": "712020:def", "createdAt": "2024-01-02T00:00:00.000Z" }
            }
        ]
    }))]);

    let comments = fetch_confluence_footer_comments(&requester, &auth_context(), "999")
        .await
        .expect("footer comments");

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].id.as_deref(), Some("111"));
    assert_eq!(comments[0].body_text, "Nice work team");
    assert_eq!(comments[0].author.as_deref(), Some("712020:abc"));
    assert_eq!(
        comments[0].created_at.as_deref(),
        Some("2024-01-01T00:00:00.000Z")
    );

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("/wiki/api/v2/pages/999/footer-comments"));
    assert!(requests[0].contains("limit=10"));
    assert!(requests[0].contains("body-format=storage"));
}

#[tokio::test]
async fn footer_comments_skip_entries_with_blank_bodies() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "results": [
            { "id": "1", "body": { "storage": { "value": "   " } }, "version": {} },
            { "id": "2", "body": { "storage": { "value": "<p>Kept</p>" } }, "version": {} }
        ]
    }))]);

    let comments = fetch_confluence_footer_comments(&requester, &auth_context(), "999")
        .await
        .expect("footer comments");

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id.as_deref(), Some("2"));
}

#[tokio::test]
async fn footer_comments_are_capped_client_side_even_if_the_server_returns_more() {
    let extra_results: Vec<Value> = (0..12)
        .map(|index| {
            json!({
                "id": index.to_string(),
                "body": { "storage": { "value": format!("<p>Comment {index}</p>") } },
                "version": {}
            })
        })
        .collect();
    let requester = FakeRequester::new(vec![Ok(json!({ "results": extra_results }))]);

    let comments = fetch_confluence_footer_comments(&requester, &auth_context(), "999")
        .await
        .expect("footer comments");

    assert_eq!(comments.len(), 10);
}

#[tokio::test]
async fn footer_comments_propagate_status_preserving_error() {
    let requester = FakeRequester::new(vec![Err(AtlassianApiError::from_status(404, "not found"))]);

    let error = fetch_confluence_footer_comments(&requester, &auth_context(), "999")
        .await
        .expect_err("404 should propagate");

    assert!(error.is_not_found());
}

#[tokio::test]
async fn attachments_map_confluence_v2_fields() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "results": [
            {
                "id": "att-1",
                "title": "diagram.png",
                "mediaType": "image/png",
                "fileSize": 2048,
                "downloadLink": "/download/attachments/999/diagram.png",
                "createdAt": "2024-02-01T00:00:00.000Z",
                "version": { "authorId": "712020:abc" }
            }
        ]
    }))]);

    let attachments = fetch_confluence_attachments(&requester, &auth_context(), "999")
        .await
        .expect("attachments");

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].filename, "diagram.png");
    assert_eq!(attachments[0].mime_type.as_deref(), Some("image/png"));
    assert_eq!(attachments[0].size, Some(2048));
    assert_eq!(
        attachments[0].content_url.as_deref(),
        Some("/download/attachments/999/diagram.png")
    );

    let requests = requester.requests();
    assert!(requests[0].contains("/wiki/api/v2/pages/999/attachments"));
    assert!(requests[0].contains("limit=25"));
}

#[tokio::test]
async fn attachments_skip_entries_missing_a_title() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "results": [
            { "id": "att-1", "mediaType": "image/png" },
            { "id": "att-2", "title": "kept.txt", "mediaType": "text/plain" }
        ]
    }))]);

    let attachments = fetch_confluence_attachments(&requester, &auth_context(), "999")
        .await
        .expect("attachments");

    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].filename, "kept.txt");
}

#[tokio::test]
async fn child_pages_map_title_and_id() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "results": [
            { "id": "555", "title": "Runbook" },
            { "id": "556", "title": "Onboarding" }
        ]
    }))]);

    let children = fetch_confluence_child_pages(&requester, &auth_context(), "999")
        .await
        .expect("children");

    assert_eq!(children.len(), 2);
    assert_eq!(children[0].key, "555");
    assert_eq!(children[0].summary, "Runbook");
    assert_eq!(children[1].key, "556");
    assert_eq!(children[1].summary, "Onboarding");

    let requests = requester.requests();
    assert!(requests[0].contains("/wiki/api/v2/pages/999/children"));
    assert!(requests[0].contains("limit=25"));
}

#[test]
fn confluence_link_content_renders_title_url_and_not_expandable_note() {
    let reference = ComposerIntegrationReference {
        title: Some("Confluence whiteboard in OPS (id 4242)".to_string()),
        url: Some("https://example.atlassian.net/wiki/spaces/OPS/whiteboard/4242".to_string()),
        ..integration_reference("confluence_link", "4242")
    };

    let content = confluence_link_content(&reference).expect("link content");

    assert_eq!(content.id, "4242");
    assert_eq!(content.title, "Confluence whiteboard in OPS (id 4242)");
    assert_eq!(
        content.url.as_deref(),
        Some("https://example.atlassian.net/wiki/spaces/OPS/whiteboard/4242")
    );
    assert!(content
        .body
        .contains("Confluence whiteboard in OPS (id 4242)"));
    assert!(content
        .body
        .contains("https://example.atlassian.net/wiki/spaces/OPS/whiteboard/4242"));
    assert!(content.body.to_ascii_lowercase().contains("not expandable"));
    assert!(content.comments.is_empty());
    assert!(content.attachments.is_empty());
    assert!(content.children.is_empty());
}

#[test]
fn confluence_link_content_falls_back_to_a_generated_title_when_none_is_set() {
    let reference = integration_reference("confluence_link", "9001");

    let content = confluence_link_content(&reference).expect("link content");

    assert!(content.title.contains("9001"));
}
