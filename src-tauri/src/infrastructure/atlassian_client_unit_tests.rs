use async_trait::async_trait;
use hyper::Method;
use serde_json::Value;
use std::sync::Mutex;

use crate::application::AtlassianApiError;

use super::atlassian_client::*;

#[test]
fn constructing_client_does_not_panic_when_roots_are_unavailable() {
    let result = std::panic::catch_unwind(HyperAtlassianApiClient::new);
    assert!(result.is_ok());
}

// ---- Test harness: fake JSON requester ----------------------------------

#[derive(Debug, Clone)]
struct RecordedRequest {
    method: Method,
    url: String,
    body: Option<Value>,
}

/// Fake [`AtlassianJsonRequester`] that records every outbound request and
/// returns a queued response. Lets us exercise the Jira helper functions
/// without performing any real network I/O.
struct FakeRequester {
    recorded: Mutex<Vec<RecordedRequest>>,
    responses: Mutex<Vec<Result<Value, String>>>,
}

impl FakeRequester {
    fn with_response(response: Value) -> Self {
        Self::with_responses(vec![response])
    }

    fn with_responses(responses: Vec<Value>) -> Self {
        Self {
            recorded: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
        }
    }

    fn recorded(&self) -> Vec<RecordedRequest> {
        self.recorded.lock().unwrap().clone()
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
        self.recorded
            .lock()
            .unwrap()
            .push(RecordedRequest { method, url, body });
        let mut responses = self.responses.lock().unwrap();
        let response = if responses.len() > 1 {
            responses.remove(0)
        } else {
            responses.first().cloned().unwrap_or(Ok(Value::Null))
        };
        response.map_err(AtlassianApiError::transport)
    }
}

fn api_token_auth() -> AtlassianAuthContext {
    AtlassianAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: AtlassianCredential::ApiToken {
            email: "user@example.com".to_string(),
            token: "api-token".to_string(),
        },
    }
}

// ---- required_trimmed ---------------------------------------------------

#[test]
fn required_trimmed_rejects_empty_string() {
    let error = required_trimmed("", "value is required").unwrap_err();
    assert_eq!(error, "value is required");
}

#[test]
fn required_trimmed_rejects_whitespace_only_string() {
    let error = required_trimmed("   \t  ", "value is required").unwrap_err();
    assert_eq!(error, "value is required");
}

#[test]
fn required_trimmed_returns_trimmed_value_for_padded_input() {
    let value = required_trimmed("  PROJ-1  ", "value is required").unwrap();
    assert_eq!(value, "PROJ-1");
}

// ---- jira_status_category -----------------------------------------------

#[test]
fn jira_status_category_maps_done_variants() {
    for key in ["done", "complete", "Closed", "RESOLVED"] {
        let status = serde_json::json!({ "statusCategory": { "key": key } });
        assert_eq!(
            jira_status_category(&status, "fallback"),
            "done",
            "key={key}"
        );
    }
}

#[test]
fn jira_status_category_maps_in_progress_variants() {
    for key in [
        "indeterminate",
        "In Progress",
        "review",
        "started",
        "active",
    ] {
        let status = serde_json::json!({ "statusCategory": { "key": key } });
        assert_eq!(
            jira_status_category(&status, "fallback"),
            "in_progress",
            "key={key}"
        );
    }
}

#[test]
fn jira_status_category_maps_todo_variants() {
    for key in ["new", "To Do", "todo", "backlog"] {
        let status = serde_json::json!({ "statusCategory": { "key": key } });
        assert_eq!(
            jira_status_category(&status, "fallback"),
            "todo",
            "key={key}"
        );
    }
}

#[test]
fn jira_status_category_defaults_to_other_for_unknown_category() {
    let status = serde_json::json!({ "statusCategory": { "key": "mysterious" } });
    assert_eq!(jira_status_category(&status, "fallback"), "other");
}

#[test]
fn jira_status_category_falls_back_to_name_when_category_missing() {
    // No statusCategory present → uses fallback_name, which here means "done".
    let status = serde_json::json!({});
    assert_eq!(jira_status_category(&status, "Done"), "done");
}

#[test]
fn jira_status_category_uses_name_key_when_key_field_missing() {
    let status = serde_json::json!({ "statusCategory": { "name": "In Progress" } });
    assert_eq!(jira_status_category(&status, "fallback"), "in_progress");
}

// ---- jira_transition_from_value -----------------------------------------

#[test]
fn jira_transition_from_value_parses_full_entry() {
    let value = serde_json::json!({
        "id": "31",
        "name": "Start Progress",
        "to": {
            "id": "3",
            "name": "In Progress",
            "statusCategory": { "key": "indeterminate" }
        }
    });
    let transition = jira_transition_from_value(&value).expect("entry should parse");
    assert_eq!(transition.provider_transition_id, "31");
    assert_eq!(transition.to_state_id, "3");
    assert_eq!(transition.name, "Start Progress");
    assert_eq!(transition.category, "in_progress");
}

#[test]
fn jira_transition_from_value_filters_null_or_missing_fields() {
    // Missing id.
    assert!(jira_transition_from_value(&serde_json::json!({
        "name": "Done",
        "to": { "id": "5" }
    }))
    .is_none());
    // Missing name.
    assert!(jira_transition_from_value(&serde_json::json!({
        "id": "1",
        "to": { "id": "5" }
    }))
    .is_none());
    // Missing to.
    assert!(jira_transition_from_value(&serde_json::json!({
        "id": "1",
        "name": "Done"
    }))
    .is_none());
    // Missing to.id.
    assert!(jira_transition_from_value(&serde_json::json!({
        "id": "1",
        "name": "Done",
        "to": { "name": "Done" }
    }))
    .is_none());
    // Null id field.
    assert!(jira_transition_from_value(&serde_json::json!({
        "id": Value::Null,
        "name": "Done",
        "to": { "id": "5" }
    }))
    .is_none());
}

#[test]
fn jira_transition_from_value_rejects_empty_trimmed_fields() {
    let value = serde_json::json!({
        "id": "   ",
        "name": "Done",
        "to": { "id": "5", "name": "Done" }
    });
    assert!(jira_transition_from_value(&value).is_none());
}

#[test]
fn jira_transition_from_value_uses_name_as_to_state_name_fallback() {
    // "to" has no "name", so the transition name is used for the category
    // classification. "Done" classifies as "done".
    let value = serde_json::json!({
        "id": "41",
        "name": "Done",
        "to": { "id": "6" }
    });
    let transition = jira_transition_from_value(&value).expect("entry should parse");
    assert_eq!(transition.category, "done");
    assert_eq!(transition.name, "Done");
}

// ---- jira_comment_adf ---------------------------------------------------

#[test]
fn jira_comment_adf_builds_expected_document_structure() {
    let adf = jira_comment_adf("Hello from RalphX");
    assert_eq!(adf["type"], "doc");
    assert_eq!(adf["version"], 1);
    let content = adf["content"].as_array().expect("content array");
    assert_eq!(content.len(), 1);
    let paragraph = &content[0];
    assert_eq!(paragraph["type"], "paragraph");
    let inner = paragraph["content"].as_array().expect("paragraph content");
    assert_eq!(inner.len(), 1);
    assert_eq!(inner[0]["type"], "text");
    assert_eq!(inner[0]["text"], "Hello from RalphX");
}

// ---- Empty-input guards (return Err before any network request) ----------

#[tokio::test]
async fn clear_jira_issue_assignee_rejects_empty_issue_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = clear_jira_issue_assignee(&requester, &api_token_auth(), "   ")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira issue key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn set_jira_issue_labels_rejects_empty_issue_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = set_jira_issue_labels(&requester, &api_token_auth(), "", vec![])
        .await
        .unwrap_err();
    assert_eq!(error, "Jira issue key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn list_jira_issue_transitions_rejects_empty_issue_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = list_jira_issue_transitions(&requester, &api_token_auth(), " ")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira issue key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn transition_jira_issue_rejects_empty_issue_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = transition_jira_issue(&requester, &api_token_auth(), "", "31")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira issue key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn transition_jira_issue_rejects_empty_transition_id() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = transition_jira_issue(&requester, &api_token_auth(), "PROJ-1", "  ")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira transition id is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn add_jira_comment_rejects_empty_issue_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = add_jira_comment(&requester, &api_token_auth(), "", "body")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira issue key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

#[tokio::test]
async fn add_jira_comment_rejects_empty_body() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = add_jira_comment(&requester, &api_token_auth(), "PROJ-1", "   ")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira comment body is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

// ---- list_jira_issue_transitions parsing & filtering --------------------

#[tokio::test]
async fn list_jira_issue_transitions_parses_and_filters_response() {
    let response = serde_json::json!({
        "transitions": [
            {
                "id": "31",
                "name": "Start Progress",
                "to": {
                    "id": "3",
                    "name": "In Progress",
                    "statusCategory": { "key": "indeterminate" }
                }
            },
            {
                // Malformed: missing "to" entirely → filtered out.
                "id": "99",
                "name": "Broken"
            },
            {
                "id": "41",
                "name": "Done",
                "to": {
                    "id": "5",
                    "name": "Done",
                    "statusCategory": { "key": "done" }
                }
            }
        ]
    });
    let requester = FakeRequester::with_response(response);
    let transitions = list_jira_issue_transitions(&requester, &api_token_auth(), "PROJ-1")
        .await
        .expect("transitions should parse");

    assert_eq!(transitions.len(), 2, "malformed entry should be filtered");
    assert_eq!(transitions[0].provider_transition_id, "31");
    assert_eq!(transitions[0].category, "in_progress");
    assert_eq!(transitions[1].provider_transition_id, "41");
    assert_eq!(transitions[1].category, "done");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::GET);
    assert!(recorded[0]
        .url
        .contains("/rest/api/3/issue/PROJ-1/transitions"));
}

#[tokio::test]
async fn list_jira_issue_transitions_returns_empty_when_field_absent() {
    let requester = FakeRequester::with_response(serde_json::json!({}));
    let transitions = list_jira_issue_transitions(&requester, &api_token_auth(), "PROJ-1")
        .await
        .expect("missing transitions field is treated as empty");
    assert!(transitions.is_empty());
}

// ---- transition_jira_issue request body ---------------------------------

#[tokio::test]
async fn transition_jira_issue_posts_expected_body() {
    let requester = FakeRequester::with_response(Value::Null);
    transition_jira_issue(&requester, &api_token_auth(), "PROJ-1", "31")
        .await
        .expect("transition should succeed");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::POST);
    assert!(recorded[0]
        .url
        .contains("/rest/api/3/issue/PROJ-1/transitions"));
    let body = recorded[0].body.as_ref().expect("request body");
    assert_eq!(body["transition"]["id"], "31");
}

// ---- set_jira_issue_labels request body ---------------------------------

#[tokio::test]
async fn set_jira_issue_labels_puts_expected_body() {
    let requester = FakeRequester::with_response(Value::Null);
    set_jira_issue_labels(
        &requester,
        &api_token_auth(),
        "PROJ-1",
        vec!["backend".to_string(), "urgent".to_string()],
    )
    .await
    .expect("label update should succeed");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::PUT);
    assert!(recorded[0].url.contains("/rest/api/3/issue/PROJ-1"));
    let body = recorded[0].body.as_ref().expect("request body");
    assert_eq!(body["fields"]["labels"][0], "backend");
    assert_eq!(body["fields"]["labels"][1], "urgent");
}

// ---- clear_jira_issue_assignee request body -----------------------------

#[tokio::test]
async fn clear_jira_issue_assignee_sends_null_account_id() {
    let requester = FakeRequester::with_response(Value::Null);
    clear_jira_issue_assignee(&requester, &api_token_auth(), "PROJ-1")
        .await
        .expect("clear assignee should succeed");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::PUT);
    assert!(recorded[0]
        .url
        .contains("/rest/api/3/issue/PROJ-1/assignee"));
    let body = recorded[0].body.as_ref().expect("request body");
    assert!(body["accountId"].is_null());
}

// ---- add_jira_comment response handling ---------------------------------

#[tokio::test]
async fn add_jira_comment_parses_created_comment_response() {
    let response = serde_json::json!({
        "id": "10001",
        "body": "Looks good to me",
        "author": { "displayName": "A. Reviewer" },
        "created": "2026-06-21T08:00:00.000+0000"
    });
    let requester = FakeRequester::with_response(response);
    let comment = add_jira_comment(&requester, &api_token_auth(), "PROJ-1", "Looks good to me")
        .await
        .expect("comment should parse");

    assert_eq!(comment.id.as_deref(), Some("10001"));
    assert_eq!(comment.author.as_deref(), Some("A. Reviewer"));
    assert_eq!(comment.body_markdown, "Looks good to me");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::POST);
    assert!(recorded[0].url.contains("/rest/api/3/issue/PROJ-1/comment"));
    // The request body is an ADF document built from the markdown.
    let body = recorded[0].body.as_ref().expect("request body");
    assert_eq!(body["body"]["type"], "doc");
    assert_eq!(
        body["body"]["content"][0]["content"][0]["text"],
        "Looks good to me"
    );
}

#[tokio::test]
async fn add_jira_comment_errors_when_response_has_no_comment_body() {
    // Response lacks a readable "body" → jira_comment_from_value returns None.
    let requester = FakeRequester::with_response(serde_json::json!({ "id": "10001" }));
    let error = add_jira_comment(&requester, &api_token_auth(), "PROJ-1", "anything")
        .await
        .unwrap_err();
    assert!(
        error.contains("did not include a readable created comment"),
        "unexpected error: {error}"
    );
}

// ---- build_jira_project_jql ---------------------------------------------

#[test]
fn build_jira_project_jql_quotes_and_orders_by_updated() {
    assert_eq!(
        build_jira_project_jql("RX"),
        "project = \"RX\" ORDER BY updated DESC"
    );
}

#[test]
fn build_jira_project_jql_escapes_quotes_and_backslashes() {
    // A project key with a quote/backslash must be escaped so the JQL stays
    // well-formed (reuses escape_search_string).
    assert_eq!(
        build_jira_project_jql("a\"b\\c"),
        "project = \"a\\\"b\\\\c\" ORDER BY updated DESC"
    );
}

// ---- list_jira_projects parsing -----------------------------------------

#[tokio::test]
async fn list_jira_projects_parses_values_and_requests_search_endpoint() {
    let response = serde_json::json!({
        "values": [
            { "id": "10000", "key": "RX", "name": "RalphX" },
            { "id": "10001", "key": "OPS", "name": "Operations" },
            // Malformed: no key → filtered out.
            { "id": "10002", "name": "No Key" }
        ]
    });
    let requester = FakeRequester::with_response(response);
    let projects = list_jira_projects(&requester, &api_token_auth(), 100)
        .await
        .expect("projects should parse");

    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].id, "10000");
    assert_eq!(projects[0].key, "RX");
    assert_eq!(projects[0].name, "RalphX");
    assert_eq!(projects[1].key, "OPS");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::GET);
    assert!(recorded[0].url.contains("/rest/api/3/project/search"));
    assert!(recorded[0].url.contains("maxResults=100"));
}

#[tokio::test]
async fn list_jira_projects_walks_project_search_pagination() {
    let first_page_values: Vec<Value> = (0..100)
        .map(|index| {
            serde_json::json!({
                "id": format!("10{index:03}"),
                "key": format!("P{index}"),
                "name": format!("Project {index}"),
            })
        })
        .collect();
    let requester = FakeRequester::with_responses(vec![
        serde_json::json!({
            "startAt": 0,
            "maxResults": 100,
            "total": 101,
            "isLast": false,
            "values": first_page_values,
        }),
        serde_json::json!({
            "startAt": 100,
            "maxResults": 100,
            "total": 101,
            "isLast": true,
            "values": [
                { "id": "10100", "key": "TAIL", "name": "Tail Project" }
            ],
        }),
    ]);

    let projects = list_jira_projects(&requester, &api_token_auth(), 101)
        .await
        .expect("projects should parse across pages");

    assert_eq!(projects.len(), 101);
    assert_eq!(
        projects.last().map(|project| project.key.as_str()),
        Some("TAIL")
    );
    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 2);
    assert!(recorded[0].url.contains("startAt=0"));
    assert!(recorded[1].url.contains("startAt=100"));
}

#[tokio::test]
async fn list_jira_projects_stops_when_project_search_reports_last_page() {
    let requester = FakeRequester::with_responses(vec![serde_json::json!({
        "startAt": 0,
        "maxResults": 2,
        "total": 10,
        "isLast": true,
        "values": [
            { "id": "10000", "key": "RX", "name": "RalphX" },
            { "id": "10001", "key": "OPS", "name": "Operations" }
        ],
    })]);

    let projects = list_jira_projects(&requester, &api_token_auth(), 5)
        .await
        .expect("projects should parse");

    assert_eq!(projects.len(), 2);
    assert_eq!(requester.recorded().len(), 1);
}

#[tokio::test]
async fn list_jira_projects_treats_exhausted_fake_responses_as_empty() {
    let requester = FakeRequester::with_responses(Vec::new());

    let projects = list_jira_projects(&requester, &api_token_auth(), 5)
        .await
        .expect("null fake response is treated as empty");

    assert!(projects.is_empty());
}

#[tokio::test]
async fn list_jira_projects_returns_empty_when_values_absent() {
    let requester = FakeRequester::with_response(serde_json::json!({}));
    let projects = list_jira_projects(&requester, &api_token_auth(), 100)
        .await
        .expect("missing values is treated as empty");
    assert!(projects.is_empty());
}

#[tokio::test]
async fn list_jira_projects_falls_back_to_key_when_id_or_name_missing() {
    let response = serde_json::json!({ "values": [ { "key": "RX" } ] });
    let requester = FakeRequester::with_response(response);
    let projects = list_jira_projects(&requester, &api_token_auth(), 100)
        .await
        .expect("projects should parse");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, "RX");
    assert_eq!(projects[0].key, "RX");
    assert_eq!(projects[0].name, "RX");
}

// ---- list_jira_project_statuses flatten + dedupe ------------------------

#[tokio::test]
async fn list_jira_project_statuses_flattens_dedupes_and_maps_category() {
    // Two issue types share "To Do" (id 1); it must appear once. Categories
    // are mapped via the existing jira_status_category helper.
    let response = serde_json::json!([
        {
            "name": "Story",
            "statuses": [
                { "id": "1", "name": "To Do", "statusCategory": { "key": "new" } },
                { "id": "3", "name": "In Progress", "statusCategory": { "key": "indeterminate" } }
            ]
        },
        {
            "name": "Bug",
            "statuses": [
                { "id": "1", "name": "To Do", "statusCategory": { "key": "new" } },
                { "id": "5", "name": "Done", "statusCategory": { "key": "done" } }
            ]
        }
    ]);
    let requester = FakeRequester::with_response(response);
    let statuses = list_jira_project_statuses(&requester, &api_token_auth(), "RX")
        .await
        .expect("statuses should parse");

    assert_eq!(statuses.len(), 3, "duplicate status id should be deduped");
    assert_eq!(statuses[0].id, "1");
    assert_eq!(statuses[0].name, "To Do");
    assert_eq!(statuses[0].category, "todo");
    assert_eq!(statuses[1].id, "3");
    assert_eq!(statuses[1].category, "in_progress");
    assert_eq!(statuses[2].id, "5");
    assert_eq!(statuses[2].category, "done");

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::GET);
    assert!(recorded[0].url.contains("/rest/api/3/project/RX/statuses"));
}

#[tokio::test]
async fn list_jira_project_statuses_returns_empty_for_empty_array() {
    let requester = FakeRequester::with_response(serde_json::json!([]));
    let statuses = list_jira_project_statuses(&requester, &api_token_auth(), "RX")
        .await
        .expect("empty array is treated as empty");
    assert!(statuses.is_empty());
}

#[tokio::test]
async fn list_jira_project_statuses_rejects_empty_project_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = list_jira_project_statuses(&requester, &api_token_auth(), "  ")
        .await
        .unwrap_err();
    assert_eq!(error, "Jira project key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}

// ---- search_jira_by_project parsing + body ------------------------------

#[tokio::test]
async fn search_jira_by_project_parses_full_issue_fields() {
    let response = serde_json::json!({
        "issues": [
            {
                "key": "RX-1",
                "fields": {
                    "summary": "Fix merge race",
                    "status": {
                        "id": "3",
                        "name": "In Progress",
                        "statusCategory": { "key": "indeterminate" }
                    },
                    "assignee": {
                        "displayName": "A. Dev",
                        "avatarUrls": { "48x48": "https://avatar/48" }
                    },
                    "labels": ["backend", "urgent"],
                    "updated": "2026-06-20T10:00:00.000+0000",
                    "priority": { "name": "High" }
                }
            }
        ]
    });
    let requester = FakeRequester::with_response(response);
    let issues = search_jira_by_project(&requester, &api_token_auth(), "RX", 40)
        .await
        .expect("issues should parse");

    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!(issue.key, "RX-1");
    assert_eq!(issue.title, "Fix merge race");
    assert_eq!(issue.status_id.as_deref(), Some("3"));
    assert_eq!(issue.status_name.as_deref(), Some("In Progress"));
    assert_eq!(issue.status_category.as_deref(), Some("in_progress"));
    assert_eq!(issue.assignee_name.as_deref(), Some("A. Dev"));
    assert_eq!(issue.assignee_avatar.as_deref(), Some("https://avatar/48"));
    assert_eq!(issue.labels, vec!["backend", "urgent"]);
    assert_eq!(
        issue.updated.as_deref(),
        Some("2026-06-20T10:00:00.000+0000")
    );
    assert_eq!(issue.priority.as_deref(), Some("High"));
    assert_eq!(
        issue.url.as_deref(),
        Some("https://example.atlassian.net/browse/RX-1")
    );

    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].method, Method::POST);
    assert!(recorded[0].url.contains("/rest/api/3/search/jql"));
    let body = recorded[0].body.as_ref().expect("request body");
    assert_eq!(body["jql"], "project = \"RX\" ORDER BY updated DESC");
    assert_eq!(body["maxResults"], 40);
    let fields = body["fields"].as_array().expect("fields array");
    assert!(fields.iter().any(|field| field == "status"));
    assert!(fields.iter().any(|field| field == "assignee"));
}

#[tokio::test]
async fn search_jira_by_project_walks_next_page_tokens() {
    let first_page_issues: Vec<Value> = (0..100)
        .map(|index| serde_json::json!({ "key": format!("RX-{index}") }))
        .collect();
    let requester = FakeRequester::with_responses(vec![
        serde_json::json!({
            "issues": first_page_issues,
            "nextPageToken": "token-2",
        }),
        serde_json::json!({
            "issues": [
                { "key": "RX-100" }
            ],
        }),
    ]);

    let issues = search_jira_by_project(&requester, &api_token_auth(), "RX", 101)
        .await
        .expect("issues should parse across token pages");

    assert_eq!(issues.len(), 101);
    assert_eq!(
        issues.last().map(|issue| issue.key.as_str()),
        Some("RX-100")
    );
    let recorded = requester.recorded();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].method, Method::POST);
    assert_eq!(
        recorded[1]
            .body
            .as_ref()
            .and_then(|body| body.get("nextPageToken"))
            .and_then(Value::as_str),
        Some("token-2"),
    );
}

#[tokio::test]
async fn search_jira_by_project_stops_on_empty_issue_page() {
    let requester = FakeRequester::with_responses(vec![serde_json::json!({
        "issues": [],
        "nextPageToken": "unused-token",
    })]);

    let issues = search_jira_by_project(&requester, &api_token_auth(), "RX", 5)
        .await
        .expect("empty page should parse");

    assert!(issues.is_empty());
    assert_eq!(requester.recorded().len(), 1);
}

#[tokio::test]
async fn search_jira_by_project_tolerates_missing_fields() {
    // Only the key is present; everything else is None/empty.
    let response = serde_json::json!({ "issues": [ { "key": "RX-2" } ] });
    let requester = FakeRequester::with_response(response);
    let issues = search_jira_by_project(&requester, &api_token_auth(), "RX", 40)
        .await
        .expect("issues should parse");

    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!(issue.key, "RX-2");
    assert_eq!(issue.title, "RX-2", "title falls back to key");
    assert!(issue.status_id.is_none());
    assert!(issue.status_category.is_none());
    assert!(issue.assignee_name.is_none());
    assert!(issue.labels.is_empty());
    assert!(issue.priority.is_none());
}

#[tokio::test]
async fn search_jira_by_project_filters_issues_without_key() {
    let response = serde_json::json!({
        "issues": [
            { "fields": { "summary": "No key" } },
            { "key": "RX-3", "fields": { "summary": "Has key" } }
        ]
    });
    let requester = FakeRequester::with_response(response);
    let issues = search_jira_by_project(&requester, &api_token_auth(), "RX", 40)
        .await
        .expect("issues should parse");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].key, "RX-3");
}

#[tokio::test]
async fn search_jira_by_project_rejects_empty_project_key() {
    let requester = FakeRequester::with_response(Value::Null);
    let error = search_jira_by_project(&requester, &api_token_auth(), "", 40)
        .await
        .unwrap_err();
    assert_eq!(error, "Jira project key is required");
    assert!(requester.recorded().is_empty(), "no request should be sent");
}
