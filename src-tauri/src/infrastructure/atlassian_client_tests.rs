use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::application::{AtlassianApiClient, AtlassianAuthContext, AtlassianCredential};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    add_jira_comment, assign_jira_issue_to_current_user, build_confluence_search_cql,
    build_jira_search_jql, clear_jira_issue_assignee, confluence_page_id_query, fetch_confluence,
    fetch_jira, list_jira_issue_transitions, search_confluence, search_jira,
    transition_jira_issue, AtlassianJsonRequester, HyperAtlassianApiClient, RequestAuth,
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
            .push(RecordedAtlassianRequest { method, url, body });
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
        summary_excerpt: None,
        include_transcript: None,
        selected_excerpt: None,
        selected_source_path: None,
        selected_range_label: None,
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
    assert!(content
        .body
        .contains("Description:\nSelected references should be valid"));
    assert!(!content.body.contains("first comment"));
    assert!(content.body.contains("second comment"));
    assert!(content.body.contains("fourth comment"));
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
    assert_eq!(content.attachments.len(), 1);
    assert_eq!(content.attachments[0].filename, "design.png");
    assert_eq!(
        content.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(content.attachments[0].size, Some(2048));
    assert_eq!(content.attachments[0].author.as_deref(), Some("Designer"));
    assert!(content.body.contains("Comment:\nPlease cover parser"));
    assert!(content.body.contains("Comment:\nAdded focused tests"));
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
