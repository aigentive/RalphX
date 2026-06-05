use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::application::{AtlassianAuthContext, AtlassianCredential};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    build_confluence_search_cql, build_jira_search_jql, confluence_page_id_query,
    fetch_confluence, fetch_jira, search_confluence, search_jira, AtlassianJsonRequester,
    RequestAuth,
};

#[derive(Clone, Debug)]
struct RecordedAtlassianRequest {
    method: Method,
    url: String,
    body: Option<Value>,
}

#[derive(Default)]
struct FakeAtlassianRequester {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<RecordedAtlassianRequest>>,
}

impl FakeAtlassianRequester {
    fn new(responses: Vec<Result<Value, String>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<RecordedAtlassianRequest> {
        self.requests.lock().expect("requests").clone()
    }
}

#[async_trait]
impl AtlassianJsonRequester for FakeAtlassianRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        _auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, String> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedAtlassianRequest {
                method,
                url,
                body,
            });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err("unexpected Atlassian request".to_string()))
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

fn jira_issue(key: &str, summary: &str) -> Value {
    json!({
        "key": key,
        "fields": {
            "summary": summary,
        }
    })
}

fn integration_reference(kind: &str, id: &str) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        key: None,
        title: None,
        url: None,
    }
}

#[test]
fn jira_search_jql_includes_accessible_closed_issues() {
    let jql = build_jira_search_jql("closed login issue").expect("jql");

    assert_eq!(jql, "text ~ \"closed login issue*\" ORDER BY updated DESC");
    assert!(!jql.to_ascii_lowercase().contains("status"));
    assert!(!jql.to_ascii_lowercase().contains("resolution"));
}

#[test]
fn jira_search_jql_uses_exact_issue_key_lookup() {
    let jql = build_jira_search_jql("rx-42").expect("jql");

    assert_eq!(jql, "issuekey = RX-42 ORDER BY updated DESC");
}

#[test]
fn confluence_search_cql_matches_page_ids_titles_and_text() {
    let cql = build_confluence_search_cql("123456");

    assert_eq!(
        cql,
        "type=page AND (id = 123456 OR title ~ \"123456*\" OR text ~ \"123456*\")"
    );
    assert_eq!(confluence_page_id_query("123456"), Some("123456"));
}

#[test]
fn confluence_search_cql_keeps_multi_word_title_queries() {
    let cql = build_confluence_search_cql("release checklist");

    assert_eq!(
        cql,
        "type=page AND (title ~ \"release checklist*\" OR text ~ \"release checklist*\")"
    );
    assert_eq!(confluence_page_id_query("release checklist"), None);
}

#[tokio::test]
async fn jira_search_exact_key_fetches_jql_and_picker_without_duplicates() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(jira_issue("PDM-81", "Exact issue")),
        Ok(json!({
            "issues": [
                jira_issue("PDM-81", "Duplicate exact issue"),
                jira_issue("PDM-82", "JQL issue")
            ]
        })),
        Ok(json!({
            "sections": [{
                "issues": [
                    { "key": "PDM-82", "summaryText": "Duplicate picker issue" },
                    { "key": "PDM-83", "summaryText": "Picker issue" }
                ]
            }]
        })),
    ]);

    let results = search_jira(&requester, &auth_context(), "pdm-81", 3)
        .await
        .expect("jira search");

    assert_eq!(
        results.iter().map(|resource| resource.id.as_str()).collect::<Vec<_>>(),
        vec!["PDM-81", "PDM-82", "PDM-83"]
    );
    assert_eq!(results[0].title, "Exact issue");
    assert_eq!(results[2].title, "Picker issue");

    let requests = requester.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/PDM-81?fields=summary,status"
    );
    assert_eq!(requests[1].method, Method::POST);
    assert_eq!(
        requests[1]
            .body
            .as_ref()
            .and_then(|body| body.get("jql"))
            .and_then(Value::as_str),
        Some("issuekey = PDM-81 ORDER BY updated DESC")
    );
    assert_eq!(
        requests[2].url,
        "https://example.atlassian.net/rest/api/3/issue/picker?query=pdm-81"
    );
}

#[tokio::test]
async fn jira_search_uses_picker_when_jql_fails_without_exact_key_result() {
    let requester = FakeAtlassianRequester::new(vec![
        Err("jql unavailable".to_string()),
        Ok(json!({
            "sections": [{
                "issues": [
                    { "key": "PDM-90", "summaryText": "Closed picker issue" }
                ]
            }]
        })),
    ]);

    let results = search_jira(&requester, &auth_context(), "closed regression", 5)
        .await
        .expect("jira picker fallback");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "PDM-90");
    assert_eq!(results[0].title, "Closed picker issue");

    let requests = requester.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, Method::POST);
    assert_eq!(
        requests[0]
            .body
            .as_ref()
            .and_then(|body| body.get("jql"))
            .and_then(Value::as_str),
        Some("text ~ \"closed regression*\" ORDER BY updated DESC")
    );
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/api/3/issue/picker?query=closed%20regression"
    );
}

#[tokio::test]
async fn confluence_search_merges_page_id_and_search_results() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(json!({
            "id": "123456",
            "title": "Runbook",
            "_links": { "webui": "/spaces/OPS/pages/123456/Runbook" }
        })),
        Ok(json!({
            "results": [
                {
                    "content": {
                        "id": "123456",
                        "title": "Duplicate runbook",
                        "_links": { "webui": "/spaces/OPS/pages/123456/Runbook" }
                    },
                    "excerpt": "<b>duplicate</b>"
                },
                {
                    "content": {
                        "id": "789",
                        "title": "Deploy notes",
                        "_links": { "webui": "/spaces/OPS/pages/789/Deploy-notes" }
                    },
                    "excerpt": "<b>Hello</b>&nbsp;&amp; world"
                }
            ]
        })),
    ]);

    let results = search_confluence(&requester, &auth_context(), "123456", 3)
        .await
        .expect("confluence search");

    assert_eq!(
        results.iter().map(|resource| resource.id.as_str()).collect::<Vec<_>>(),
        vec!["123456", "789"]
    );
    assert_eq!(results[0].title, "Runbook");
    assert_eq!(results[1].excerpt.as_deref(), Some("Hello & world"));
    assert_eq!(
        results[1].url.as_deref(),
        Some("https://example.atlassian.net/wiki/spaces/OPS/pages/789/Deploy-notes")
    );

    let requests = requester.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/wiki/api/v2/pages/123456"
    );
    assert!(requests[1]
        .url
        .contains("https://example.atlassian.net/wiki/rest/api/search?cql="));
    assert!(requests[1].url.contains("id%20%3D%20123456"));
}

#[tokio::test]
async fn confluence_search_returns_page_id_result_when_cql_search_fails() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(json!({
            "id": "123456",
            "title": "Runbook",
            "_links": { "webui": "/spaces/OPS/pages/123456/Runbook" }
        })),
        Err("search unavailable".to_string()),
    ]);

    let results = search_confluence(&requester, &auth_context(), "123456", 3)
        .await
        .expect("confluence direct page fallback");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "123456");
    assert_eq!(requester.requests().len(), 2);
}

#[tokio::test]
async fn fetch_jira_renders_issue_fields_and_recent_comments() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Fix reference search",
            "status": { "name": "Done" },
            "updated": "2026-06-05T10:00:00.000+0000",
            "description": "Selected references should be valid",
            "comment": {
                "comments": [
                    { "body": "first comment" },
                    { "body": "second comment" },
                    { "body": "third comment" },
                    { "body": "fourth comment" }
                ]
            }
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("PDM-81".to_string()),
            title: Some("Fallback title".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
    )
    .await
    .expect("jira fetch");

    assert_eq!(content.id, "PDM-81");
    assert_eq!(content.title, "Fix reference search");
    assert!(content.body.contains("Key: PDM-81"));
    assert!(content.body.contains("Status: Done"));
    assert!(content.body.contains("Description:\nSelected references should be valid"));
    assert!(!content.body.contains("first comment"));
    assert!(content.body.contains("second comment"));
    assert!(content.body.contains("fourth comment"));
}

#[tokio::test]
async fn fetch_confluence_strips_storage_html_and_builds_web_url() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "title": "Reference docs",
        "body": {
            "storage": {
                "value": "<p>Hello&nbsp;&amp; <strong>team</strong></p>"
            }
        },
        "_links": { "webui": "/spaces/OPS/pages/456/Reference-docs" }
    }))]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("confluence fetch");

    assert_eq!(content.id, "456");
    assert_eq!(content.title, "Reference docs");
    assert_eq!(content.body, "Hello & team");
    assert_eq!(
        content.url.as_deref(),
        Some("https://example.atlassian.net/wiki/spaces/OPS/pages/456/Reference-docs")
    );
}
