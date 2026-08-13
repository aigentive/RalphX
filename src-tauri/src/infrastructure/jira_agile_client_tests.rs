use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::domain::integrations::AtlassianApiError;

use super::atlassian_client::{
    AtlassianJsonRequester, JiraAgileAuthContext, JiraAgileCredential, RequestAuth,
};
use super::jira_agile_client::{
    get_jira_board_configuration, list_jira_active_sprints, list_jira_boards,
    list_jira_sprint_issues,
};

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: Method,
    url: String,
}

#[derive(Default)]
struct FakeRequester {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl FakeRequester {
    fn new(responses: Vec<Result<Value, String>>) -> Self {
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
impl AtlassianJsonRequester for FakeRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        _auth: RequestAuth<'_>,
        _body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest { method, url });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err("unexpected Jira request".to_string()))
            .map_err(AtlassianApiError::transport)
    }
}

fn auth_context() -> JiraAgileAuthContext {
    JiraAgileAuthContext {
        site_url: "https://example.atlassian.net".to_string(),
        credential: JiraAgileCredential::ApiToken {
            email: "dev@example.com".to_string(),
            token: "token".to_string(),
        },
    }
}

#[tokio::test]
async fn list_jira_boards_walks_pages_and_preserves_distinct_boards() {
    let requester = FakeRequester::new(vec![
        Ok(json!({
            "startAt": 0,
            "maxResults": 1,
            "isLast": false,
            "values": [{
                "id": 41,
                "name": "Delivery Scrum",
                "type": "scrum",
                "location": { "projectKey": "RX" }
            }]
        })),
        Ok(json!({
            "startAt": 1,
            "maxResults": 1,
            "isLast": true,
            "values": [{
                "id": 73,
                "name": "Delivery Kanban",
                "type": "kanban",
                "location": { "projectKey": "RX" }
            }]
        })),
    ]);

    let boards = list_jira_boards(&requester, &auth_context(), " RX ")
        .await
        .expect("list Jira boards");

    assert_eq!(
        boards
            .iter()
            .map(|board| board.id.as_str())
            .collect::<Vec<_>>(),
        vec!["41", "73"]
    );
    assert_eq!(boards[0].name, "Delivery Scrum");
    assert_eq!(boards[1].board_type, "kanban");
    assert_eq!(boards[1].project_key.as_deref(), Some("RX"));

    let requests = requester.requests();
    assert_eq!(requests.len(), 2);
    assert!(requests.iter().all(|request| request.method == Method::GET));
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/agile/1.0/board?projectKeyOrId=RX&startAt=0&maxResults=50"
    );
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/agile/1.0/board?projectKeyOrId=RX&startAt=1&maxResults=50"
    );
}

#[tokio::test]
async fn get_jira_board_configuration_keeps_ordered_multi_status_columns() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "id": 41,
        "name": "Delivery Scrum",
        "type": "scrum",
        "columnConfig": {
            "columns": [
                {
                    "name": "To Do",
                    "statuses": [{ "id": "10000" }]
                },
                {
                    "name": "In Progress",
                    "statuses": [
                        { "id": "3" },
                        { "id": "4" },
                        { "id": "5" }
                    ]
                },
                {
                    "name": "Done",
                    "statuses": [{ "id": "6" }]
                }
            ]
        }
    }))]);

    let configuration = get_jira_board_configuration(&requester, &auth_context(), "41")
        .await
        .expect("get Jira board configuration");

    assert_eq!(configuration.id, "41");
    assert_eq!(configuration.name, "Delivery Scrum");
    assert_eq!(configuration.board_type, "scrum");
    assert_eq!(
        configuration
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        vec!["To Do", "In Progress", "Done"]
    );
    assert_eq!(configuration.columns[1].status_ids, vec!["3", "4", "5"]);
    assert_eq!(
        requester.requests()[0].url,
        "https://example.atlassian.net/rest/agile/1.0/board/41/configuration"
    );
}

#[tokio::test]
async fn list_jira_active_sprints_walks_pages_and_keeps_duplicate_names_by_id() {
    let requester = FakeRequester::new(vec![
        Ok(json!({
            "startAt": 0,
            "maxResults": 1,
            "isLast": false,
            "values": [{
                "id": 91,
                "state": "active",
                "name": "Iteration 1",
                "startDate": "2026-07-01T09:00:00.000Z",
                "endDate": "2026-07-15T09:00:00.000Z",
                "originBoardId": 41,
                "goal": "Ship alpha"
            }]
        })),
        Ok(json!({
            "startAt": 1,
            "maxResults": 1,
            "isLast": true,
            "values": [{
                "id": 92,
                "state": "active",
                "name": "Iteration 1",
                "originBoardId": 41
            }]
        })),
    ]);

    let sprints = list_jira_active_sprints(&requester, &auth_context(), "41")
        .await
        .expect("list active Jira sprints");

    assert_eq!(sprints.len(), 2);
    assert_eq!(sprints[0].id, "91");
    assert_eq!(sprints[1].id, "92");
    assert_eq!(sprints[0].name, sprints[1].name);
    assert_eq!(sprints[0].board_id, "41");
    assert_eq!(sprints[0].goal.as_deref(), Some("Ship alpha"));

    let requests = requester.requests();
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/agile/1.0/board/41/sprint?state=active&startAt=0&maxResults=50"
    );
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/agile/1.0/board/41/sprint?state=active&startAt=1&maxResults=50"
    );
}

#[tokio::test]
async fn list_jira_boards_rejects_malformed_page_instead_of_returning_empty_success() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "startAt": 0,
        "isLast": true
    }))]);

    let result = list_jira_boards(&requester, &auth_context(), "RX").await;

    assert_eq!(
        result,
        Err("Jira board response was missing values".to_string())
    );
}

#[tokio::test]
async fn list_jira_boards_with_a_blank_project_key_lists_every_board() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "startAt": 0,
        "isLast": true,
        "values": [{
            "id": 41,
            "name": "Delivery Scrum",
            "type": "scrum",
            "location": { "projectKey": "RX" }
        }]
    }))]);

    let boards = list_jira_boards(&requester, &auth_context(), "  ")
        .await
        .expect("list Jira boards");

    assert_eq!(boards.len(), 1);
    assert_eq!(
        requester.requests()[0].url,
        "https://example.atlassian.net/rest/agile/1.0/board?startAt=0&maxResults=50"
    );
}

#[tokio::test]
async fn list_jira_sprint_issues_walks_pages_and_returns_enriched_summaries() {
    let requester = FakeRequester::new(vec![
        Ok(json!({
            "startAt": 0,
            "maxResults": 1,
            "total": 2,
            "issues": [{
                "key": "RX-1",
                "fields": {
                    "summary": "Fix the thing",
                    "status": { "name": "In Progress" },
                    "issuetype": { "name": "Bug" },
                    "assignee": { "displayName": "A. Dev" },
                    "updated": "2026-08-01T10:00:00.000+0000"
                }
            }]
        })),
        Ok(json!({
            "startAt": 1,
            "maxResults": 1,
            "total": 2,
            "issues": [{
                "key": "RX-2",
                "fields": {
                    "summary": "Ship the thing",
                    "status": { "name": "To Do" }
                }
            }]
        })),
    ]);

    let issues = list_jira_sprint_issues(&requester, &auth_context(), "91", 50)
        .await
        .expect("list Jira sprint issues");

    assert_eq!(
        issues.iter().map(|i| i.key.clone()).collect::<Vec<_>>(),
        vec![Some("RX-1".to_string()), Some("RX-2".to_string())]
    );
    assert_eq!(issues[0].status.as_deref(), Some("In Progress"));
    assert_eq!(issues[0].issue_type.as_deref(), Some("Bug"));
    assert_eq!(issues[0].assignee.as_deref(), Some("A. Dev"));
    assert_eq!(
        issues[0].updated_at.as_deref(),
        Some("2026-08-01T10:00:00.000+0000")
    );
    assert_eq!(issues[1].status.as_deref(), Some("To Do"));
    assert_eq!(issues[1].assignee, None);

    let requests = requester.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].url,
        "https://example.atlassian.net/rest/agile/1.0/sprint/91/issue?fields=summary,status,issuetype,assignee,updated&startAt=0&maxResults=50"
    );
    assert_eq!(
        requests[1].url,
        "https://example.atlassian.net/rest/agile/1.0/sprint/91/issue?fields=summary,status,issuetype,assignee,updated&startAt=1&maxResults=50"
    );
}

#[tokio::test]
async fn list_jira_sprint_issues_stops_once_the_requested_limit_is_reached() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "startAt": 0,
        "maxResults": 2,
        "isLast": false,
        "issues": [
            { "key": "RX-1", "fields": { "summary": "One", "status": { "name": "To Do" } } },
            { "key": "RX-2", "fields": { "summary": "Two", "status": { "name": "To Do" } } }
        ]
    }))]);

    let issues = list_jira_sprint_issues(&requester, &auth_context(), "91", 1)
        .await
        .expect("list Jira sprint issues");

    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].key.as_deref(), Some("RX-1"));
    // Reaching the caller's limit stops pagination without requesting the next page.
    assert_eq!(requester.requests().len(), 1);
}

#[tokio::test]
async fn list_jira_sprint_issues_rejects_malformed_page_instead_of_returning_empty_success() {
    let requester = FakeRequester::new(vec![Ok(json!({
        "startAt": 0,
        "isLast": true
    }))]);

    let result = list_jira_sprint_issues(&requester, &auth_context(), "91", 50).await;

    assert_eq!(
        result,
        Err("Jira sprint issue response was missing issues".to_string())
    );
}
