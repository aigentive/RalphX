use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::domain::integrations::{
    AtlassianApiError, AtlassianRawMethod, ConfluencePageCreateRequest,
    ConfluencePageUpdateRequest, JiraIssueCreateRequest, JiraIssueUpdateRequest,
};

use super::atlassian_client::{
    AtlassianAuthContext, AtlassianCredential, AtlassianJsonRequester, AtlassianResourceKind,
    RequestAuth,
};
use crate::domain::integrations::validate_atlassian_raw_path as validate_raw_path;

use super::atlassian_mcp_client::{
    confluence_create_page, confluence_get_page, confluence_update_page, create_jira_issue,
    raw_api_request, update_jira_issue,
};

/// Expected ADF document for a description with no markdown structure.
fn plain_text_adf(text: &str) -> Value {
    json!({
        "type": "doc",
        "version": 1,
        "content": [{
            "type": "paragraph",
            "content": [{ "type": "text", "text": text }]
        }]
    })
}

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: Method,
    url: String,
    body: Option<Value>,
}

#[derive(Default)]
struct FakeRequester {
    requests: Mutex<Vec<RecordedRequest>>,
    responses: Mutex<VecDeque<Result<Value, AtlassianApiError>>>,
}

impl FakeRequester {
    fn new(responses: Vec<Result<Value, AtlassianApiError>>) -> Self {
        Self {
            requests: Mutex::new(Vec::new()),
            responses: Mutex::new(VecDeque::from(responses)),
        }
    }

    fn ok(responses: Vec<Value>) -> Self {
        Self::new(responses.into_iter().map(Ok).collect())
    }

    fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait]
impl AtlassianJsonRequester for FakeRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        _auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest { method, url, body });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err(AtlassianApiError::transport("unexpected Atlassian request")))
    }
}

fn api_token_auth() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::ApiToken {
            email: "dev@example.com".to_string(),
            token: "api-token".to_string(),
        },
    }
}

fn oauth_auth() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::OAuth {
            access_token: "access-token".to_string(),
            cloud_id: "cloud-1".to_string(),
        },
    }
}

// ---- Jira write ---------------------------------------------------------

#[tokio::test]
async fn create_jira_issue_posts_v3_fields_and_wraps_description_in_adf() {
    let requester = FakeRequester::ok(vec![json!({ "id": "10101", "key": "PROJ-7" })]);
    let request = JiraIssueCreateRequest {
        project_key: "PROJ".to_string(),
        issue_type: "Task".to_string(),
        summary: "Wire the tier gate".to_string(),
        description: Some("Plain text body".to_string()),
        labels: vec!["ralphx".to_string()],
        priority: Some("High".to_string()),
        ..JiraIssueCreateRequest::default()
    };

    let created = create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect("issue should be created");

    assert_eq!(created.key, "PROJ-7");
    assert_eq!(created.id, "10101");
    assert_eq!(created.url, "https://example.atlassian.net/browse/PROJ-7");

    let recorded = requester.requests();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::POST);
    assert_eq!(
        recorded[0].url,
        "https://example.atlassian.net/rest/api/3/issue"
    );
    let fields = recorded[0].body.as_ref().unwrap().get("fields").unwrap();
    assert_eq!(fields.get("project").unwrap(), &json!({ "key": "PROJ" }));
    assert_eq!(fields.get("issuetype").unwrap(), &json!({ "name": "Task" }));
    assert_eq!(fields.get("summary").unwrap(), "Wire the tier gate");
    assert_eq!(fields.get("labels").unwrap(), &json!(["ralphx"]));
    assert_eq!(fields.get("priority").unwrap(), &json!({ "name": "High" }));
    assert_eq!(
        fields.get("description").unwrap(),
        &plain_text_adf("Plain text body")
    );
}

#[tokio::test]
async fn create_jira_issue_maps_parent_key_assignee_and_components() {
    let requester = FakeRequester::ok(vec![json!({ "id": "10101", "key": "PROJ-7" })]);
    let request = JiraIssueCreateRequest {
        project_key: "PROJ".to_string(),
        issue_type: "Story".to_string(),
        summary: "Wire the richer create path".to_string(),
        parent_key: Some("PROJ-1".to_string()),
        assignee_account_id: Some("account-9".to_string()),
        components: vec!["Backend".to_string(), "API".to_string()],
        ..JiraIssueCreateRequest::default()
    };

    create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect("issue should be created");

    let recorded = requester.requests();
    let fields = recorded[0].body.as_ref().unwrap().get("fields").unwrap();
    assert_eq!(fields.get("parent").unwrap(), &json!({ "key": "PROJ-1" }));
    assert_eq!(
        fields.get("assignee").unwrap(),
        &json!({ "accountId": "account-9" })
    );
    assert_eq!(
        fields.get("components").unwrap(),
        &json!([{ "name": "Backend" }, { "name": "API" }])
    );
}

#[tokio::test]
async fn create_jira_issue_omits_parent_assignee_and_components_when_absent() {
    let requester = FakeRequester::ok(vec![json!({ "id": "10101", "key": "PROJ-7" })]);
    let request = JiraIssueCreateRequest {
        project_key: "PROJ".to_string(),
        issue_type: "Task".to_string(),
        summary: "Minimal issue".to_string(),
        ..JiraIssueCreateRequest::default()
    };

    create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect("issue should be created");

    let recorded = requester.requests();
    let fields = recorded[0].body.as_ref().unwrap().get("fields").unwrap();
    assert!(fields.get("parent").is_none());
    assert!(fields.get("assignee").is_none());
    assert!(fields.get("components").is_none());
}

#[tokio::test]
async fn create_jira_issue_renders_markdown_description_via_the_writer() {
    // Regression: description must go through markdown_to_adf, not
    // plain_text_adf, so headings/lists render as real ADF nodes.
    let requester = FakeRequester::ok(vec![json!({ "id": "10101", "key": "PROJ-7" })]);
    let request = JiraIssueCreateRequest {
        project_key: "PROJ".to_string(),
        issue_type: "Task".to_string(),
        summary: "Wire the writer".to_string(),
        description: Some("## Context\n\n- one\n- two".to_string()),
        ..JiraIssueCreateRequest::default()
    };

    create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect("issue should be created");

    let recorded = requester.requests();
    let description = recorded[0]
        .body
        .as_ref()
        .unwrap()
        .get("fields")
        .unwrap()
        .get("description")
        .unwrap();
    let content = description["content"].as_array().expect("content array");
    assert_eq!(content[0]["type"], "heading");
    assert_eq!(content[0]["attrs"]["level"], 2);
    assert_eq!(content[1]["type"], "bulletList");
}

#[tokio::test]
async fn update_jira_issue_renders_markdown_description_via_the_writer() {
    let requester = FakeRequester::ok(vec![Value::Null]);
    let request = JiraIssueUpdateRequest {
        description: Some("**bold** text".to_string()),
        ..JiraIssueUpdateRequest::default()
    };

    update_jira_issue(&requester, &api_token_auth(), "PROJ-7", &request)
        .await
        .expect("update should succeed");

    let recorded = requester.requests();
    let description = recorded[0]
        .body
        .as_ref()
        .unwrap()
        .get("fields")
        .unwrap()
        .get("description")
        .unwrap();
    let span = &description["content"][0]["content"][0];
    assert_eq!(span["text"], "bold");
    assert_eq!(span["marks"][0]["type"], "strong");
}

#[tokio::test]
async fn create_jira_issue_rejects_missing_required_fields_before_any_request() {
    let requester = FakeRequester::ok(vec![json!({ "key": "PROJ-1" })]);
    let request = JiraIssueCreateRequest {
        project_key: "  ".to_string(),
        issue_type: "Task".to_string(),
        summary: "Summary".to_string(),
        ..JiraIssueCreateRequest::default()
    };

    let error = create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect_err("blank project key should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn update_jira_issue_sends_only_the_supplied_fields() {
    let requester = FakeRequester::ok(vec![Value::Null]);
    let request = JiraIssueUpdateRequest {
        summary: Some("New summary".to_string()),
        labels: Some(vec!["a".to_string(), "b".to_string()]),
        ..JiraIssueUpdateRequest::default()
    };

    update_jira_issue(&requester, &api_token_auth(), "PROJ-7", &request)
        .await
        .expect("update should succeed");

    let recorded = requester.requests();
    assert_eq!(recorded[0].method, Method::PUT);
    assert_eq!(
        recorded[0].url,
        "https://example.atlassian.net/rest/api/3/issue/PROJ-7"
    );
    let fields = recorded[0].body.as_ref().unwrap().get("fields").unwrap();
    assert_eq!(fields.get("summary").unwrap(), "New summary");
    assert_eq!(fields.get("labels").unwrap(), &json!(["a", "b"]));
    assert!(fields.get("description").is_none());
    assert!(fields.get("priority").is_none());
}

#[tokio::test]
async fn update_jira_issue_rejects_an_empty_patch() {
    let requester = FakeRequester::ok(vec![Value::Null]);

    let error = update_jira_issue(
        &requester,
        &api_token_auth(),
        "PROJ-7",
        &JiraIssueUpdateRequest::default(),
    )
    .await
    .expect_err("empty update should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

// ---- Confluence ---------------------------------------------------------

fn confluence_page_payload(version: i64) -> Value {
    json!({
        "id": "12345",
        "title": "Runbook",
        "spaceId": "789",
        "version": { "number": version },
        "body": { "storage": { "value": "<p>current</p>" } },
        "_links": { "webui": "/spaces/ENG/pages/12345/Runbook" }
    })
}

#[tokio::test]
async fn confluence_get_page_requests_storage_format_and_parses_the_body() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(4)]);

    let page = confluence_get_page(&requester, &api_token_auth(), "12345")
        .await
        .expect("page should be read");

    assert_eq!(page.id, "12345");
    assert_eq!(page.title, "Runbook");
    assert_eq!(page.space_id.as_deref(), Some("789"));
    assert_eq!(page.version, 4);
    assert_eq!(page.body_storage, "<p>current</p>");
    assert_eq!(
        page.url.as_deref(),
        Some("https://example.atlassian.net/wiki/spaces/ENG/pages/12345/Runbook")
    );

    let recorded = requester.requests();
    assert_eq!(recorded[0].method, Method::GET);
    assert_eq!(
        recorded[0].url,
        "https://example.atlassian.net/wiki/api/v2/pages/12345?body-format=storage"
    );
}

#[tokio::test]
async fn confluence_create_page_posts_storage_representation() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(1)]);
    let request = ConfluencePageCreateRequest {
        space_id: "789".to_string(),
        title: "Runbook".to_string(),
        body_storage: Some("<p>new</p>".to_string()),
        parent_id: Some("111".to_string()),
        ..ConfluencePageCreateRequest::default()
    };

    confluence_create_page(&requester, &api_token_auth(), &request)
        .await
        .expect("page should be created");

    let recorded = requester.requests();
    assert_eq!(recorded[0].method, Method::POST);
    assert_eq!(
        recorded[0].url,
        "https://example.atlassian.net/wiki/api/v2/pages"
    );
    let body = recorded[0].body.as_ref().unwrap();
    assert_eq!(body.get("spaceId").unwrap(), "789");
    assert_eq!(body.get("parentId").unwrap(), "111");
    assert_eq!(
        body.get("body").unwrap(),
        &json!({ "representation": "storage", "value": "<p>new</p>" })
    );
}

#[tokio::test]
async fn confluence_create_page_converts_markdown_to_storage_xhtml() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(1)]);
    let request = ConfluencePageCreateRequest {
        space_id: "789".to_string(),
        title: "Runbook".to_string(),
        body_markdown: Some("# Title\n\n- one\n- two".to_string()),
        ..ConfluencePageCreateRequest::default()
    };

    confluence_create_page(&requester, &api_token_auth(), &request)
        .await
        .expect("page should be created");

    let recorded = requester.requests();
    let body = recorded[0].body.as_ref().unwrap();
    let storage = body["body"]["value"].as_str().expect("storage string");
    assert_eq!(storage, "<h1>Title</h1><ul><li>one</li><li>two</li></ul>");
}

#[tokio::test]
async fn confluence_create_page_rejects_neither_storage_nor_markdown() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(1)]);
    let request = ConfluencePageCreateRequest {
        space_id: "789".to_string(),
        title: "Runbook".to_string(),
        ..ConfluencePageCreateRequest::default()
    };

    let error = confluence_create_page(&requester, &api_token_auth(), &request)
        .await
        .expect_err("missing body should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn confluence_create_page_rejects_both_storage_and_markdown() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(1)]);
    let request = ConfluencePageCreateRequest {
        space_id: "789".to_string(),
        title: "Runbook".to_string(),
        body_storage: Some("<p>storage</p>".to_string()),
        body_markdown: Some("markdown".to_string()),
        ..ConfluencePageCreateRequest::default()
    };

    let error = confluence_create_page(&requester, &api_token_auth(), &request)
        .await
        .expect_err("supplying both should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn confluence_update_page_reads_the_current_version_and_increments_it() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(4), confluence_page_payload(5)]);
    let request = ConfluencePageUpdateRequest {
        body_storage: Some("<p>updated</p>".to_string()),
        ..ConfluencePageUpdateRequest::default()
    };

    let page = confluence_update_page(&requester, &api_token_auth(), "12345", &request)
        .await
        .expect("page should be updated");

    assert_eq!(page.version, 5);
    let recorded = requester.requests();
    assert_eq!(recorded.len(), 2, "read-modify-write");
    assert_eq!(recorded[0].method, Method::GET);
    assert_eq!(recorded[1].method, Method::PUT);
    let body = recorded[1].body.as_ref().unwrap();
    assert_eq!(body.get("version").unwrap(), &json!({ "number": 5 }));
    // Title was not supplied, so the current title is preserved.
    assert_eq!(body.get("title").unwrap(), "Runbook");
    assert_eq!(
        body.get("body").unwrap(),
        &json!({ "representation": "storage", "value": "<p>updated</p>" })
    );
}

#[tokio::test]
async fn confluence_update_page_converts_markdown_to_storage_xhtml() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(4), confluence_page_payload(5)]);
    let request = ConfluencePageUpdateRequest {
        body_markdown: Some("**bold**".to_string()),
        ..ConfluencePageUpdateRequest::default()
    };

    confluence_update_page(&requester, &api_token_auth(), "12345", &request)
        .await
        .expect("page should be updated");

    let recorded = requester.requests();
    let body = recorded[1].body.as_ref().unwrap();
    assert_eq!(
        body.get("body").unwrap(),
        &json!({ "representation": "storage", "value": "<p><strong>bold</strong></p>" })
    );
}

#[tokio::test]
async fn confluence_update_page_rejects_both_storage_and_markdown_before_any_request() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(4)]);
    let request = ConfluencePageUpdateRequest {
        body_storage: Some("<p>storage</p>".to_string()),
        body_markdown: Some("markdown".to_string()),
        ..ConfluencePageUpdateRequest::default()
    };

    let error = confluence_update_page(&requester, &api_token_auth(), "12345", &request)
        .await
        .expect_err("supplying both should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn confluence_update_page_rejects_an_empty_patch_before_reading() {
    let requester = FakeRequester::ok(vec![confluence_page_payload(1)]);

    let error = confluence_update_page(
        &requester,
        &api_token_auth(),
        "12345",
        &ConfluencePageUpdateRequest::default(),
    )
    .await
    .expect_err("empty update should be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

// ---- Escape-hatch containment -------------------------------------------

#[test]
fn raw_path_validation_accepts_every_allowlisted_prefix() {
    for path in [
        "/rest/api/3/issue/PROJ-1",
        "/rest/agile/1.0/board/5/sprint",
        "/wiki/rest/api/search?cql=type=page",
        "/wiki/api/v2/pages/1?body-format=storage",
    ] {
        assert!(validate_raw_path(path).is_ok(), "{path} should be accepted");
    }
}

#[test]
fn raw_path_validation_rejects_absolute_urls_and_host_injection() {
    for path in [
        "https://evil.example.com/rest/api/3/issue",
        "http://evil.example.com/rest/api/3/issue",
        "//evil.example.com/rest/api/3/issue",
        "\\\\evil.example.com\\rest\\api",
        "rest/api/3/issue",
        "",
    ] {
        assert!(
            validate_raw_path(path).is_err(),
            "{path:?} must be rejected, not sanitized"
        );
    }
}

#[test]
fn raw_path_validation_rejects_parent_directory_segments() {
    for path in [
        "/rest/api/../../secrets",
        "/rest/api/3/../../../etc/passwd",
        "/wiki/api/v2/pages/..",
    ] {
        assert!(validate_raw_path(path).is_err(), "{path} must be rejected");
    }
}

#[test]
fn raw_path_validation_rejects_percent_encoded_traversal() {
    for path in [
        "/rest/api/3/%2e%2e%2f%2e%2e%2fplugins/servlet/admin",
        "/rest/api/3/%2E%2E/secrets",
        "/wiki/api/v2/pages/%2f%2fetc",
    ] {
        assert!(validate_raw_path(path).is_err(), "{path} must be rejected");
    }
}

#[test]
fn raw_path_validation_still_accepts_percent_encoding_in_the_query_string() {
    assert!(validate_raw_path("/wiki/rest/api/search?cql=type%3Dpage").is_ok());
}

#[test]
fn raw_path_validation_rejects_non_allowlisted_prefixes() {
    for path in [
        "/plugins/servlet/admin",
        "/rest/internal/2/whatever",
        "/wiki/plugins/servlet/admin",
    ] {
        assert!(validate_raw_path(path).is_err(), "{path} must be rejected");
    }
}

#[tokio::test]
async fn raw_api_request_never_forwards_a_body_on_read_only_methods() {
    let requester = FakeRequester::ok(vec![json!({ "ok": true })]);

    raw_api_request(
        &requester,
        &api_token_auth(),
        AtlassianRawMethod::Get,
        AtlassianResourceKind::Jira,
        "/rest/agile/1.0/board/5/sprint",
        Some(json!({ "ignored": true })),
    )
    .await
    .expect("GET should succeed");

    let recorded = requester.requests();
    assert_eq!(recorded[0].method, Method::GET);
    assert!(recorded[0].body.is_none());
}

#[tokio::test]
async fn raw_api_request_routes_oauth_mode_through_the_cloud_id_host() {
    let requester = FakeRequester::ok(vec![json!({ "ok": true })]);

    raw_api_request(
        &requester,
        &oauth_auth(),
        AtlassianRawMethod::Get,
        AtlassianResourceKind::Confluence,
        "/wiki/api/v2/pages/1",
        None,
    )
    .await
    .expect("GET should succeed");

    assert_eq!(
        requester.requests()[0].url,
        "https://api.atlassian.com/ex/confluence/cloud-1/wiki/api/v2/pages/1"
    );
}

#[tokio::test]
async fn raw_api_request_rejects_unsafe_paths_before_issuing_a_request() {
    let requester = FakeRequester::ok(vec![json!({ "ok": true })]);

    let error = raw_api_request(
        &requester,
        &api_token_auth(),
        AtlassianRawMethod::Get,
        AtlassianResourceKind::Jira,
        "https://evil.example.com/rest/api/3/issue",
        None,
    )
    .await
    .expect_err("absolute URL must be rejected");

    assert_eq!(error.status, None);
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn raw_api_request_bounds_the_response_size() {
    let oversized = json!({ "blob": "x".repeat(300 * 1024) });
    let requester = FakeRequester::ok(vec![oversized]);

    let error = raw_api_request(
        &requester,
        &api_token_auth(),
        AtlassianRawMethod::Get,
        AtlassianResourceKind::Jira,
        "/rest/api/3/search",
        None,
    )
    .await
    .expect_err("oversized response must be rejected");

    assert_eq!(error.status, None);
    assert!(error.to_string().contains("byte limit"));
}

// ---- Typed status propagation -------------------------------------------

#[tokio::test]
async fn write_operations_surface_the_numeric_status_of_atlassian_failures() {
    let requester = FakeRequester::new(vec![Err(AtlassianApiError::from_status(
        429,
        "{\"message\":\"Rate limit exceeded\"}",
    ))]);
    let request = JiraIssueCreateRequest {
        project_key: "PROJ".to_string(),
        issue_type: "Task".to_string(),
        summary: "Summary".to_string(),
        ..JiraIssueCreateRequest::default()
    };

    let error = create_jira_issue(&requester, &api_token_auth(), &request)
        .await
        .expect_err("rate limited create should fail");

    assert_eq!(error.status, Some(429));
    assert!(error.is_rate_limited());
}

#[tokio::test]
async fn read_operations_surface_a_missing_page_as_a_404() {
    let requester = FakeRequester::new(vec![Err(AtlassianApiError::from_status(
        404,
        "page not found",
    ))]);

    let error = confluence_get_page(&requester, &api_token_auth(), "99999")
        .await
        .expect_err("missing page should fail");

    assert_eq!(error.status, Some(404));
    assert!(error.is_not_found());
}
