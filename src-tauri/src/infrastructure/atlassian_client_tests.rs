use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::domain::integrations::{
    AtlassianApiClient, AtlassianApiError, AtlassianAuthContext, AtlassianCredential,
};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    add_jira_comment, assign_jira_issue_to_account, assign_jira_issue_to_current_user,
    build_confluence_search_cql, build_jira_search_jql, clear_jira_issue_assignee,
    confluence_page_id_query, fetch_confluence, fetch_jira, list_confluence_spaces,
    list_jira_comments, list_jira_issue_transitions, search_confluence, search_confluence_raw,
    search_jira, search_jira_raw, search_jira_users, transition_jira_issue,
    AtlassianJsonRequester, HyperAtlassianApiClient, RequestAuth,
};

#[derive(Clone, Debug)]
struct RecordedAtlassianRequest {
    method: Method,
    url: String,
    body: Option<Value>,
}

#[derive(Default)]
struct FakeAtlassianRequester {
    responses: Mutex<VecDeque<Result<Value, AtlassianApiError>>>,
    requests: Mutex<Vec<RecordedAtlassianRequest>>,
}

impl FakeAtlassianRequester {
    fn new(responses: Vec<Result<Value, AtlassianApiError>>) -> Self {
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
    ) -> Result<Value, AtlassianApiError> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedAtlassianRequest { method, url, body });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err(AtlassianApiError::transport("unexpected Atlassian request")))
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
        summary_excerpt: None,
        include_transcript: None,
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

// ---- Raw JQL/CQL pass-through (gap G1) -----------------------------------

#[tokio::test]
async fn search_jira_raw_submits_the_caller_jql_byte_identical() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({ "issues": [] }))]);
    let jql = "project = ENG AND status = \"In Progress\"";

    search_jira_raw(&requester, &auth_context(), jql, 25)
        .await
        .expect("raw jql search should succeed");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1, "no smart-mode fallback request");
    assert_eq!(requests[0].method, Method::POST);
    assert!(requests[0].url.contains("/rest/api/3/search/jql"));
    assert_eq!(
        requests[0]
            .body
            .as_ref()
            .and_then(|body| body.get("jql"))
            .and_then(Value::as_str),
        Some(jql),
        "raw JQL must reach the request body unmodified"
    );
}

#[tokio::test]
async fn search_jira_raw_rejects_a_blank_query_without_any_request() {
    let requester = FakeAtlassianRequester::new(vec![]);

    let error = search_jira_raw(&requester, &auth_context(), "   ", 25)
        .await
        .expect_err("blank raw JQL should be rejected");

    assert!(error.message.contains("must not be blank"));
    assert!(requester.requests().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn search_jira_smart_mode_still_rewrites_free_text_into_jql() {
    // Regression: adding the raw pass-through path must not change smart
    // mode's existing issue-key/phrase JQL rewriting.
    let requester = FakeAtlassianRequester::new(vec![
        Ok(json!({ "issues": [] })),
        Ok(json!({ "sections": [] })),
    ]);

    search_jira(&requester, &auth_context(), "closed login issue", 25)
        .await
        .expect("smart search should succeed");

    let requests = requester.requests();
    let jql_request = requests
        .iter()
        .find(|request| request.method == Method::POST)
        .expect("a jql request should have been sent");
    assert_eq!(
        jql_request
            .body
            .as_ref()
            .and_then(|body| body.get("jql"))
            .and_then(Value::as_str),
        Some("text ~ \"closed login issue*\" ORDER BY updated DESC")
    );
}

#[tokio::test]
async fn search_confluence_raw_submits_the_caller_cql_byte_identical() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({ "results": [] }))]);
    let cql = "text ~ \"release notes\"";

    search_confluence_raw(&requester, &auth_context(), cql, 25)
        .await
        .expect("raw cql search should succeed");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1, "no page-id short-circuit request");
    assert_eq!(requests[0].method, Method::GET);
    assert!(requests[0]
        .url
        .contains("cql=text%20~%20%22release%20notes%22"));
    assert!(
        !requests[0].url.contains("type%3Dpage"),
        "raw CQL must not be wrapped by the smart-mode type=page query"
    );
}

#[tokio::test]
async fn search_confluence_raw_rejects_a_blank_query_without_any_request() {
    let requester = FakeAtlassianRequester::new(vec![]);

    let error = search_confluence_raw(&requester, &auth_context(), "  ", 25)
        .await
        .expect_err("blank raw CQL should be rejected");

    assert!(error.message.contains("must not be blank"));
    assert!(requester.requests().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn hyper_requester_surfaces_invalid_urls_without_network() {
    let client = HyperAtlassianApiClient::new().expect("client");
    let mut auth = auth_context();
    auth.site_url = "not a valid url".to_string();

    let result = client.validate(&auth).await;

    assert_eq!(
        result,
        Err("Atlassian credentials did not validate for Jira or Confluence".to_string())
    );
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
        results
            .iter()
            .map(|resource| resource.id.as_str())
            .collect::<Vec<_>>(),
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
        Err(AtlassianApiError::transport("jql unavailable")),
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
async fn jira_assign_to_current_user_puts_my_account_id_on_issue() {
    let requester =
        FakeAtlassianRequester::new(vec![Ok(json!({ "accountId": "abc-123" })), Ok(Value::Null)]);

    assign_jira_issue_to_current_user(&requester, &auth_context(), " rx-42 ")
        .await
        .expect("assign Jira issue");

    let requests = requester.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/myself"
    );
    assert_eq!(requests[1].method, Method::PUT);
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/api/3/issue/rx-42/assignee"
    );
    assert_eq!(
        requests[1].body.as_ref(),
        Some(&json!({ "accountId": "abc-123" }))
    );
}

#[tokio::test]
async fn jira_clear_assignee_puts_null_account_id_on_issue() {
    let requester = FakeAtlassianRequester::new(vec![Ok(Value::Null)]);

    clear_jira_issue_assignee(&requester, &auth_context(), " rx-42 ")
        .await
        .expect("clear Jira assignee");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::PUT);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/rx-42/assignee"
    );
    assert_eq!(
        requests[0].body.as_ref(),
        Some(&json!({ "accountId": Value::Null }))
    );
}

#[tokio::test]
async fn jira_assign_to_account_puts_the_given_account_id_on_issue() {
    let requester = FakeAtlassianRequester::new(vec![Ok(Value::Null)]);

    assign_jira_issue_to_account(&requester, &auth_context(), " rx-42 ", " account-9 ")
        .await
        .expect("assign Jira issue to account");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::PUT);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/rx-42/assignee"
    );
    assert_eq!(
        requests[0].body.as_ref(),
        Some(&json!({ "accountId": "account-9" }))
    );
}

#[tokio::test]
async fn jira_assign_to_account_rejects_a_blank_account_id_without_any_request() {
    let requester = FakeAtlassianRequester::new(vec![Ok(Value::Null)]);

    let error = assign_jira_issue_to_account(&requester, &auth_context(), "RX-42", "  ")
        .await
        .expect_err("blank accountId should be rejected");

    assert_eq!(error, "Jira accountId is required");
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn jira_search_users_bounds_max_results_to_twenty_and_parses_matches() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!([
        { "accountId": "acc-1", "displayName": "Ada Lovelace" },
        { "accountId": "acc-2", "displayName": "  " },
        { "accountId": "  " },
    ]))]);

    let users = search_jira_users(&requester, &auth_context(), "ada", 500)
        .await
        .expect("search jira users");

    // A blank accountId is dropped; a blank displayName falls back to the id.
    assert_eq!(users.len(), 2);
    assert_eq!(users[0].account_id, "acc-1");
    assert_eq!(users[0].display_name, "Ada Lovelace");
    assert_eq!(users[1].account_id, "acc-2");
    assert_eq!(users[1].display_name, "acc-2");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/user/search?query=ada&maxResults=20"
    );
}

#[tokio::test]
async fn jira_search_users_rejects_a_blank_query_without_any_request() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!([]))]);

    let error = search_jira_users(&requester, &auth_context(), "  ", 10)
        .await
        .expect_err("blank query should be rejected");

    assert_eq!(error, "Jira user search query is required");
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn jira_lists_comments_with_the_providers_true_total() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "total": 12,
        "comments": [
            {
                "id": "c1",
                "author": { "displayName": "Reviewer" },
                "created": "2026-06-17T10:01:00.000+0000",
                "body": "Please cover parser"
            }
        ]
    }))]);

    let page = list_jira_comments(&requester, &auth_context(), " RX-42 ", 0, 20)
        .await
        .expect("list jira comments");

    assert_eq!(page.total, 12);
    assert_eq!(page.comments.len(), 1);
    assert_eq!(page.comments[0].id.as_deref(), Some("c1"));
    assert_eq!(page.comments[0].body_markdown, "Please cover parser");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/RX-42/comment?startAt=0&maxResults=20&orderBy=-created"
    );
}

#[tokio::test]
async fn jira_lists_comments_rejects_a_blank_issue_key_without_any_request() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({}))]);

    let error = list_jira_comments(&requester, &auth_context(), "  ", 0, 20)
        .await
        .expect_err("blank issue key should be rejected");

    assert_eq!(error, "Jira issue key is required");
    assert!(requester.requests().is_empty());
}

#[tokio::test]
async fn confluence_lists_spaces_from_the_v2_api() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "results": [
            { "id": "10001", "key": "ENG", "name": "Engineering" },
            { "id": "10002", "key": "OPS", "name": "Operations" },
        ]
    }))]);

    let spaces = list_confluence_spaces(&requester, &auth_context(), 500)
        .await
        .expect("list confluence spaces");

    assert_eq!(spaces.len(), 2);
    assert_eq!(spaces[0].id, "10001");
    assert_eq!(spaces[0].key, "ENG");
    assert_eq!(spaces[0].name, "Engineering");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    // Limit clamps to the API-safe upper bound rather than the caller's 500.
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/wiki/api/v2/spaces?limit=250"
    );
}

#[tokio::test]
async fn jira_lists_workflow_transitions_for_issue() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "transitions": [
            {
                "id": "31",
                "name": "Start Progress",
                "to": {
                    "id": "3",
                    "name": "In Progress",
                    "statusCategory": { "key": "indeterminate" }
                }
            }
        ]
    }))]);

    let transitions = list_jira_issue_transitions(&requester, &auth_context(), " RX-42 ")
        .await
        .expect("list transitions");

    assert_eq!(transitions.len(), 1);
    assert_eq!(transitions[0].provider_transition_id, "31");
    assert_eq!(transitions[0].to_state_id, "3");
    assert_eq!(transitions[0].name, "Start Progress");
    assert_eq!(transitions[0].category, "in_progress");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/RX-42/transitions"
    );
}

#[tokio::test]
async fn jira_transition_posts_workflow_transition_id() {
    let requester = FakeAtlassianRequester::new(vec![Ok(Value::Null)]);

    transition_jira_issue(&requester, &auth_context(), "RX-42", "31")
        .await
        .expect("transition issue");

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::POST);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/RX-42/transitions"
    );
    assert_eq!(
        requests[0].body.as_ref(),
        Some(&json!({ "transition": { "id": "31" } }))
    );
}

#[tokio::test]
async fn jira_add_comment_posts_adf_and_returns_created_comment() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "id": "10001",
        "author": { "displayName": "A. User" },
        "body": "Ready for review",
        "created": "2026-06-20T08:00:00.000+0000",
        "updated": "2026-06-20T08:00:00.000+0000"
    }))]);

    let comment = add_jira_comment(&requester, &auth_context(), "RX-42", "Ready for review")
        .await
        .expect("add comment");

    assert_eq!(comment.id.as_deref(), Some("10001"));
    assert_eq!(comment.body_markdown, "Ready for review");
    assert_eq!(comment.author.as_deref(), Some("A. User"));

    let requests = requester.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, Method::POST);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/api/3/issue/RX-42/comment"
    );
    let body = requests[0].body.as_ref().expect("comment body");
    assert_eq!(
        body.pointer("/body/content/0/content/0/text")
            .and_then(Value::as_str),
        Some("Ready for review")
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
        results
            .iter()
            .map(|resource| resource.id.as_str())
            .collect::<Vec<_>>(),
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
        Err(AtlassianApiError::transport("search unavailable")),
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
        &[],
    )
    .await
    .expect("jira fetch");

    assert_eq!(content.id, "PDM-81");
    assert_eq!(content.title, "Fix reference search");
    assert!(content.body.contains("Key: PDM-81"));
    assert!(content.body.contains("Status: Done"));
    assert!(content
        .body
        .contains("Description:\nSelected references should be valid"));
    assert!(content
        .body
        .contains("Comment by Jira user (unknown date):\nfirst comment"));
    assert!(content.body.contains("second comment"));
    assert!(content.body.contains("fourth comment"));
    assert!(!content.body.contains("older comments omitted"));
    // No-attachments case: the fixture carries no "attachment" field, so the
    // rendered body must omit the whole attachments section.
    assert!(content.attachments.is_empty());
    assert!(!content.body.contains("Attachments"));
}

#[tokio::test]
async fn fetch_jira_parses_adf_description_comments_and_attachments() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Build Jira tab",
            "status": { "name": "In Review" },
            "assignee": { "displayName": "Ada Lovelace" },
            "reporter": { "displayName": "Grace Hopper" },
            "updated": "2026-06-17T10:00:00.000+0000",
            "issuetype": { "name": "Task" },
            "labels": ["backend", "urgent"],
            "priority": { "name": "High" },
            "description": {
                "type": "doc",
                "content": [
                    {
                        "type": "paragraph",
                        "content": [
                            { "type": "text", "text": "Render " },
                            {
                                "type": "text",
                                "text": "rich Jira",
                                "marks": [{ "type": "strong" }]
                            }
                        ]
                    },
                    {
                        "type": "heading",
                        "attrs": { "level": 2 },
                        "content": [{ "type": "text", "text": "Acceptance Criteria" }]
                    },
                    {
                        "type": "bulletList",
                        "content": [
                            {
                                "type": "listItem",
                                "content": [{
                                    "type": "paragraph",
                                    "content": [{ "type": "text", "text": "Primary ticket is visible" }]
                                }]
                            },
                            {
                                "type": "listItem",
                                "content": [{
                                    "type": "paragraph",
                                    "content": [
                                        { "type": "text", "text": "Agents receive " },
                                        {
                                            "type": "text",
                                            "text": "prompt context",
                                            "marks": [{ "type": "code" }]
                                        }
                                    ]
                                }]
                            }
                        ]
                    },
                    {
                        "type": "heading",
                        "attrs": { "level": 2 },
                        "content": [{ "type": "text", "text": "Notes:" }]
                    },
                    {
                        "type": "paragraph",
                        "content": [{ "type": "text", "text": "Not part of AC" }]
                    }
                ]
            },
            "comment": {
                "comments": [
                    {
                        "id": "c1",
                        "author": { "displayName": "Reviewer" },
                        "created": "2026-06-17T10:01:00.000+0000",
                        "updated": "2026-06-17T10:02:00.000+0000",
                        "body": {
                            "type": "doc",
                            "content": [{
                                "type": "paragraph",
                                "content": [{ "type": "text", "text": "Please cover parser" }]
                            }]
                        }
                    },
                    {
                        "id": "c2",
                        "author": { "displayName": "Implementer" },
                        "body": {
                            "type": "doc",
                            "content": [{
                                "type": "paragraph",
                                "content": [{ "type": "text", "text": "Added focused tests" }]
                            }]
                        }
                    }
                ]
            },
            "attachment": [
                {
                    "id": "a1",
                    "filename": "design.png",
                    "mimeType": "image/png",
                    "size": 2048,
                    "author": { "displayName": "Designer" },
                    "content": "https://example.atlassian.net/secure/attachment/a1/design.png",
                    "thumbnail": "https://example.atlassian.net/secure/thumbnail/a1",
                    "created": "2026-06-17T10:03:00.000+0000"
                },
                {
                    "id": "a2",
                    "filename": "spec.pdf",
                    "mimeType": "application/pdf",
                    "size": 512000,
                    "content": "https://example.atlassian.net/secure/attachment/a2/spec.pdf",
                    "created": "2026-06-17T10:04:00.000+0000"
                },
                {
                    "id": "a3",
                    "filename": "notes.txt",
                    "mimeType": "text/plain",
                    "size": 128,
                    "created": "2026-06-17T10:05:00.000+0000"
                },
                {
                    "id": "a4",
                    "filename": "   "
                }
            ]
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-42".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert_eq!(content.title, "Build Jira tab");
    assert_eq!(content.status.as_deref(), Some("In Review"));
    assert_eq!(content.assignee.as_deref(), Some("Ada Lovelace"));
    assert_eq!(content.reporter.as_deref(), Some("Grace Hopper"));
    assert_eq!(
        content.updated_at_remote.as_deref(),
        Some("2026-06-17T10:00:00.000+0000")
    );
    assert_eq!(
        content.description_markdown.as_deref(),
        Some(
            "Render **rich Jira**\n## Acceptance Criteria\n- Primary ticket is visible\n- Agents receive `prompt context`\n## Notes:\nNot part of AC"
        )
    );
    assert_eq!(
        content.description_text.as_deref(),
        Some(
            "Render rich Jira\nAcceptance Criteria\nPrimary ticket is visible\nAgents receive prompt context\nNotes:\nNot part of AC"
        )
    );
    assert_eq!(
        content.acceptance_criteria_markdown.as_deref(),
        Some("- Primary ticket is visible\n- Agents receive `prompt context`")
    );
    assert_eq!(
        content.acceptance_criteria_text.as_deref(),
        Some("Primary ticket is visible\nAgents receive prompt context")
    );
    assert_eq!(content.comments.len(), 2);
    assert_eq!(content.comments[0].id.as_deref(), Some("c1"));
    assert_eq!(content.comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(content.comments[0].body_text, "Please cover parser");
    assert_eq!(
        content.comments[0].updated_at.as_deref(),
        Some("2026-06-17T10:02:00.000+0000")
    );
    assert_eq!(
        content.issue_type.as_deref(),
        Some("Task"),
        "1.3: issue type parsed from the extended field list"
    );
    assert_eq!(content.labels, vec!["backend", "urgent"]);
    assert_eq!(content.priority.as_deref(), Some("High"));
    // 3 attachments render, filtering out the blank-filename entry.
    assert_eq!(content.attachments.len(), 3);
    assert_eq!(content.attachments[0].filename, "design.png");
    assert_eq!(
        content.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(content.attachments[0].size, Some(2048));
    assert_eq!(content.attachments[0].author.as_deref(), Some("Designer"));
    assert_eq!(content.attachments[1].filename, "spec.pdf");
    assert_eq!(content.attachments[2].filename, "notes.txt");
    assert!(content
        .body
        .contains("Comment by Reviewer (2026-06-17T10:02:00.000+0000):\nPlease cover parser"));
    assert!(content
        .body
        .contains("Comment by Implementer (unknown date):\nAdded focused tests"));
    // Rendered reference metadata (gap G7): previously fetched but discarded
    // fields now surface in the prompt body.
    assert!(content.body.contains("Type: Task"));
    assert!(content.body.contains("Assignee: Ada Lovelace"));
    assert!(content.body.contains("Reporter: Grace Hopper"));
    assert!(content.body.contains("Priority: High"));
    assert!(content.body.contains("Labels: backend, urgent"));
    assert!(content.body.contains("Attachments (3):"));
    assert!(content.body.contains("- design.png (image/png, 2 KB)"));
    assert!(content
        .body
        .contains("- spec.pdf (application/pdf, 500 KB)"));
    assert!(content.body.contains("- notes.txt (text/plain, 128 B)"));
    assert!(content
        .body
        .contains("(readable via list_ticket_attachments / fetch_ticket_attachment)"));
    // Attachment filenames/mime/size render, but never the download URLs.
    assert!(!content
        .body
        .contains("https://example.atlassian.net/secure"));
}

#[tokio::test]
async fn fetch_jira_requests_custom_acceptance_criteria_and_prefers_it_to_description() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Use the custom field",
            "description": "## Acceptance Criteria\n\n- Description fallback",
            "customfield_10037": {
                "type": "doc",
                "content": [{
                    "type": "bulletList",
                    "content": [{
                        "type": "listItem",
                        "content": [{
                            "type": "paragraph",
                            "content": [{ "type": "text", "text": "Custom field wins" }]
                        }]
                    }]
                }]
            }
        }
    }))]);
    let custom_fields = vec!["customfield_10037".to_string()];

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-101".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &custom_fields,
    )
    .await
    .expect("jira fetch");

    assert!(requester.requests()[0].url.contains(
        "fields=summary,status,description,assignee,reporter,updated,comment,attachment,issuetype,labels,priority,parent,subtasks,customfield_10037"
    ));
    assert_eq!(
        content.acceptance_criteria_markdown.as_deref(),
        Some("- Custom field wins")
    );
    assert_eq!(
        content.acceptance_criteria_text.as_deref(),
        Some("Custom field wins")
    );
    let acceptance_offset = content
        .body
        .find("Acceptance Criteria:")
        .expect("acceptance criteria in prompt body");
    let description_offset = content
        .body
        .find("Description:")
        .expect("description in prompt body");
    assert!(acceptance_offset < description_offset);
}

#[tokio::test]
async fn fetch_jira_renders_five_newest_comments_with_metadata_and_omitted_count() {
    let comments = (1..=8)
        .map(|index| {
            json!({
                "id": format!("c{index}"),
                "author": { "displayName": format!("Author {index}") },
                "created": format!("2026-06-{index:02}T10:00:00Z"),
                "body": format!("comment body {index}")
            })
        })
        .collect::<Vec<_>>();
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Comment bounds",
            "comment": { "comments": comments }
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-102".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert!(!content.body.contains("comment body 3"));
    assert!(content
        .body
        .contains("Comment by Author 4 (2026-06-04T10:00:00Z):\ncomment body 4"));
    assert!(content.body.contains("comment body 8"));
    // No "total" field in the fixture: falls back to the fetched-comment count.
    assert!(content
        .body
        .contains("(8 total comments; showing latest 5 — jira_list_comments for more)"));
    assert_eq!(content.body.matches("Comment by ").count(), 5);
}

#[tokio::test]
async fn fetch_jira_comment_count_hint_uses_the_providers_true_total_not_the_fetched_page_size() {
    // Jira's `comment.total` can exceed the (already-capped-at-10) fetched
    // comments array when an issue has many more comments than fit in one
    // page. The hint must report the provider's true total, not len().
    let comments = (1..=6)
        .map(|index| json!({ "id": format!("c{index}"), "body": format!("comment {index}") }))
        .collect::<Vec<_>>();
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Many comments",
            "comment": { "comments": comments, "total": 42 }
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-200".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert!(content
        .body
        .contains("(42 total comments; showing latest 5 — jira_list_comments for more)"));
}

#[tokio::test]
async fn fetch_jira_omits_the_comment_count_hint_when_total_is_within_the_shown_limit() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Few comments",
            "comment": {
                "comments": [{ "body": "only one" }],
                "total": 1
            }
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-201".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert!(!content.body.contains("jira_list_comments"));
    assert!(!content.body.contains("total comments"));
}

#[tokio::test]
async fn fetch_jira_renders_parent_and_subtasks_from_returned_fields_with_zero_extra_calls() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Child work item",
            "status": { "name": "In Progress" },
            "issuetype": { "name": "Story" },
            "parent": {
                "key": "RX-1",
                "fields": {
                    "summary": "Umbrella epic",
                    "status": { "name": "In Progress" },
                    "issuetype": { "name": "Epic" }
                }
            },
            "subtasks": [
                {
                    "key": "RX-10",
                    "fields": {
                        "summary": "Write tests",
                        "status": { "name": "To Do" },
                        "issuetype": { "name": "Sub-task" }
                    }
                },
                {
                    "key": "RX-11",
                    "fields": {
                        "summary": "Implement",
                        "status": { "name": "Done" },
                        "issuetype": { "name": "Sub-task" }
                    }
                }
            ]
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-5".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    // 3.1: a non-epic issue with a parent/subtasks never makes the epic
    // children lookup — only the base issue GET.
    assert_eq!(
        requester.requests().len(),
        1,
        "non-epic issue makes zero extra calls"
    );
    assert!(content.body.contains("Parent: RX-1 — Umbrella epic"));
    assert!(content.body.contains("Subtasks (2):"));
    assert!(content.body.contains("- RX-10 — Write tests (To Do)"));
    assert!(content.body.contains("- RX-11 — Implement (Done)"));
    assert_eq!(content.parent_key.as_deref(), Some("RX-1"));
    assert!(content.children.is_empty());
    assert!(!content.body.contains("Child issues"));
}

#[tokio::test]
async fn fetch_jira_renders_parent_key_only_when_parent_summary_is_missing() {
    let requester = FakeAtlassianRequester::new(vec![Ok(json!({
        "fields": {
            "summary": "Bare parent link",
            "parent": { "key": "RX-2" }
        }
    }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-6".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert_eq!(content.parent_key.as_deref(), Some("RX-2"));
    assert!(content.body.contains("Parent: RX-2"));
    assert!(!content.body.contains("Parent: RX-2 — "));
}

#[tokio::test]
async fn fetch_jira_epic_renders_child_issues_with_exactly_one_extra_call() {
    let epic_issue = json!({
        "fields": {
            "summary": "Umbrella epic",
            "status": { "name": "In Progress" },
            "issuetype": { "name": "Epic" }
        }
    });
    let children_response = json!({
        "issues": [
            {
                "key": "RX-21",
                "fields": {
                    "summary": "Child 1",
                    "status": { "name": "To Do" },
                    "issuetype": { "name": "Task" }
                }
            },
            {
                "key": "RX-22",
                "fields": {
                    "summary": "Child 2",
                    "status": { "name": "Done" },
                    "issuetype": { "name": "Task" }
                }
            }
        ],
        "total": 30
    });
    let requester = FakeAtlassianRequester::new(vec![Ok(epic_issue), Ok(children_response)]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-1".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    let requests = requester.requests();
    assert_eq!(requests.len(), 2, "epic issue makes exactly one extra call");
    assert_eq!(requests[1].method, Method::POST);
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/api/3/search/jql"
    );
    let body = requests[1]
        .body
        .as_ref()
        .expect("epic children request body");
    assert_eq!(
        body.get("jql").and_then(Value::as_str),
        Some("parent = RX-1 ORDER BY rank")
    );
    assert_eq!(
        body.get("fields").and_then(Value::as_array),
        Some(&vec![
            Value::String("summary".to_string()),
            Value::String("status".to_string()),
            Value::String("issuetype".to_string()),
        ])
    );
    assert_eq!(body.get("maxResults").and_then(Value::as_u64), Some(25));
    assert!(content.body.contains("Child issues (2 shown of 30):"));
    assert!(content.body.contains("- RX-21 — Child 1 (To Do)"));
    assert!(content.body.contains("- RX-22 — Child 2 (Done)"));
    assert_eq!(content.children.len(), 2);
    assert_eq!(content.children[0].key, "RX-21");
}

#[tokio::test]
async fn fetch_jira_epic_matches_issue_type_case_insensitively() {
    let epic_issue = json!({
        "fields": {
            "summary": "lowercase epic",
            "issuetype": { "name": "epic" }
        }
    });
    let requester = FakeAtlassianRequester::new(vec![Ok(epic_issue), Ok(json!({ "issues": [] }))]);

    fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-1".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    assert_eq!(requester.requests().len(), 2);
}

#[tokio::test]
async fn fetch_jira_epic_caps_child_issues_at_twenty_five_even_if_server_overreturns() {
    let epic_issue = json!({
        "fields": {
            "summary": "Big epic",
            "issuetype": { "name": "Epic" }
        }
    });
    let issues: Vec<Value> = (1..=30)
        .map(|index| {
            json!({
                "key": format!("RX-{}", 100 + index),
                "fields": {
                    "summary": format!("Child {index}"),
                    "status": { "name": "To Do" }
                }
            })
        })
        .collect();
    let requester =
        FakeAtlassianRequester::new(vec![Ok(epic_issue), Ok(json!({ "issues": issues }))]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-1".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("jira fetch");

    // Defensive cap in case the server ever returns more than the requested
    // maxResults; "shown of M" reports the real returned population (30).
    assert_eq!(content.children.len(), 25);
    assert!(content.body.contains("Child issues (25 shown of 30):"));
}

#[tokio::test]
async fn fetch_jira_epic_child_call_failure_still_renders_body() {
    let epic_issue = json!({
        "fields": {
            "summary": "Umbrella epic",
            "status": { "name": "In Progress" },
            "issuetype": { "name": "Epic" }
        }
    });
    let requester = FakeAtlassianRequester::new(vec![
        Ok(epic_issue),
        Err(AtlassianApiError::transport("boom")),
    ]);

    let content = fetch_jira(
        &requester,
        &auth_context(),
        &ComposerIntegrationReference {
            key: Some("RX-1".to_string()),
            ..integration_reference("jira", "ignored-id")
        },
        &[],
    )
    .await
    .expect("secondary-call failure must not fail the whole expansion");

    assert_eq!(requester.requests().len(), 2);
    assert_eq!(content.title, "Umbrella epic");
    assert!(content.body.contains("Status: In Progress"));
    assert!(content.body.contains("Child issues: unavailable"));
    assert!(content.children.is_empty());
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
    assert!(content.comments.is_empty());
    assert!(content.attachments.is_empty());
    assert!(content.children.is_empty());
}

fn confluence_page_value() -> Value {
    json!({
        "title": "Reference docs",
        "body": { "storage": { "value": "<p>Hello team</p>" } },
        "_links": { "webui": "/spaces/OPS/pages/456/Reference-docs" }
    })
}

fn confluence_footer_comments_value(count: usize) -> Value {
    let results: Vec<Value> = (0..count)
        .map(|index| {
            json!({
                "id": format!("comment-{index}"),
                "body": { "storage": { "value": format!("<p>Comment {index}</p>") } },
                "version": { "authorId": format!("author-{index}"), "createdAt": "2024-01-01T00:00:00.000Z" }
            })
        })
        .collect();
    json!({ "results": results })
}

fn confluence_attachments_value() -> Value {
    json!({
        "results": [{
            "id": "att-1",
            "title": "diagram.png",
            "mediaType": "image/png",
            "fileSize": 2048,
            "downloadLink": "/download/attachments/456/diagram.png"
        }]
    })
}

fn confluence_children_value() -> Value {
    json!({
        "results": [
            { "id": "789", "title": "Child page A" },
            { "id": "790", "title": "Child page B" }
        ]
    })
}

#[tokio::test]
async fn fetch_confluence_populates_comments_attachments_and_children_from_secondary_calls() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(confluence_page_value()),
        Ok(confluence_footer_comments_value(3)),
        Ok(confluence_attachments_value()),
        Ok(confluence_children_value()),
    ]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("confluence fetch");

    assert_eq!(content.comments.len(), 3);
    assert_eq!(content.attachments.len(), 1);
    assert_eq!(content.attachments[0].filename, "diagram.png");
    assert_eq!(content.children.len(), 2);
    assert_eq!(content.children[0].key, "789");
    assert_eq!(content.children[0].summary, "Child page A");
    assert!(content.body.contains("Attachments (1):"));
    assert!(content.body.contains("diagram.png"));
    assert!(content.body.contains("Child pages (2):"));
    assert!(content.body.contains("Comment 0"));

    let requests = requester.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[1].url.contains("/footer-comments"));
    assert!(requests[2].url.contains("/attachments"));
    assert!(requests[3].url.contains("/children"));
}

#[tokio::test]
async fn fetch_confluence_renders_only_the_last_five_comments_in_the_body() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(confluence_page_value()),
        Ok(confluence_footer_comments_value(8)),
        Err(AtlassianApiError::transport("no attachments fixture")),
        Err(AtlassianApiError::transport("no children fixture")),
    ]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("confluence fetch");

    assert_eq!(content.comments.len(), 8);
    assert!(content.body.contains("Comment 7"));
    assert!(content.body.contains("Comment 3"));
    assert!(!content.body.contains("Comment 2"));
    assert!(content.body.contains("(3 older comments omitted)"));
}

#[tokio::test]
async fn fetch_confluence_survives_footer_comments_failure_and_keeps_other_sections() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(confluence_page_value()),
        Err(AtlassianApiError::from_status(500, "boom")),
        Ok(confluence_attachments_value()),
        Ok(confluence_children_value()),
    ]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("page fetch must still succeed when comments fail");

    assert!(content.comments.is_empty());
    assert_eq!(content.attachments.len(), 1);
    assert_eq!(content.children.len(), 2);
}

#[tokio::test]
async fn fetch_confluence_survives_attachments_failure_and_keeps_other_sections() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(confluence_page_value()),
        Ok(confluence_footer_comments_value(2)),
        Err(AtlassianApiError::from_status(
            404,
            "no attachments endpoint",
        )),
        Ok(confluence_children_value()),
    ]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("page fetch must still succeed when attachments fail");

    assert_eq!(content.comments.len(), 2);
    assert!(content.attachments.is_empty());
    assert_eq!(content.children.len(), 2);
    assert!(!content.body.contains("Attachments ("));
}

#[tokio::test]
async fn fetch_confluence_survives_child_pages_failure_and_keeps_other_sections() {
    let requester = FakeAtlassianRequester::new(vec![
        Ok(confluence_page_value()),
        Ok(confluence_footer_comments_value(2)),
        Ok(confluence_attachments_value()),
        Err(AtlassianApiError::from_status(403, "forbidden")),
    ]);

    let content = fetch_confluence(
        &requester,
        &auth_context(),
        &integration_reference("confluence", "456"),
    )
    .await
    .expect("page fetch must still succeed when child pages fail");

    assert_eq!(content.comments.len(), 2);
    assert_eq!(content.attachments.len(), 1);
    assert!(content.children.is_empty());
    assert!(!content.body.contains("Child pages ("));
}

#[tokio::test]
async fn the_requester_seam_preserves_the_numeric_status_of_a_failed_call() {
    let requester = FakeAtlassianRequester::new(vec![
        Err(AtlassianApiError::from_status(
            429,
            "{\"message\":\"Rate limit exceeded\"}",
        )),
        Err(AtlassianApiError::from_status(404, "Issue does not exist")),
    ]);
    let auth = auth_context();

    let rate_limited = requester
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                &auth,
                crate::domain::integrations::AtlassianResourceKind::Jira,
                "/rest/api/3/issue/PROJ-1",
            ),
            RequestAuth::None,
            None,
        )
        .await
        .expect_err("rate limited request should fail");
    assert_eq!(rate_limited.status, Some(429));
    assert!(rate_limited.is_rate_limited());
    assert!(!rate_limited.is_not_found());

    let missing = requester
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                &auth,
                crate::domain::integrations::AtlassianResourceKind::Jira,
                "/rest/api/3/issue/PROJ-404",
            ),
            RequestAuth::None,
            None,
        )
        .await
        .expect_err("missing issue should fail");
    assert_eq!(missing.status, Some(404));
    assert!(missing.is_not_found());
    assert!(!missing.is_rate_limited());
}

#[tokio::test]
async fn legacy_string_callers_still_receive_the_status_in_the_message() {
    let requester = FakeAtlassianRequester::new(vec![Err(AtlassianApiError::from_status(429, ""))]);
    let auth = auth_context();

    // Callers that still return `Result<_, String>` keep compiling through the
    // `From<AtlassianApiError> for String` conversion, and the rendered message
    // still names the status.
    let error: String = search_jira(&requester, &auth, "anything", 5)
        .await
        .expect_err("failed search should surface an error");
    assert_eq!(error, "Atlassian returned HTTP 429");
}
