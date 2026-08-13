use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::domain::integrations::AtlassianApiError;
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    AtlassianJsonRequester, JiraAgileAuthContext, JiraAgileCredential, RequestAuth,
};
use super::jira_board_context_client::fetch_jira_board_context;

#[derive(Default)]
struct FakeRequester {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<String>>,
}

impl FakeRequester {
    fn new(responses: Vec<Result<Value, String>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            requests: Mutex::new(Vec::new()),
        }
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
            .unwrap_or_else(|| Err("unexpected Jira board request".to_string()))
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

fn board_reference() -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: "atlassian".to_string(),
        kind: "jira_board".to_string(),
        id: "12".to_string(),
        key: None,
        title: None,
        url: Some("https://example.atlassian.net/jira/software/projects/RX/boards/12".to_string()),
        summary_excerpt: None,
        include_transcript: None,
    }
}

fn board_configuration_response() -> Value {
    json!({
        "id": 12,
        "name": "RX Board",
        "type": "scrum",
        "columnConfig": {
            "columns": [
                { "name": "To Do", "statuses": [{ "id": "1" }] },
                { "name": "In Progress", "statuses": [{ "id": "3" }] },
                { "name": "Done", "statuses": [{ "id": "5" }] }
            ]
        }
    })
}

#[tokio::test]
async fn fetch_jira_board_context_renders_no_active_sprint_message() {
    let requester = FakeRequester::new(vec![
        Ok(board_configuration_response()),
        Ok(json!({ "values": [], "isLast": true })),
    ]);

    let content = fetch_jira_board_context(&requester, &auth_context(), &board_reference())
        .await
        .expect("board context should resolve");

    assert_eq!(content.id, "12");
    assert_eq!(content.title, "Board: RX Board");
    assert!(
        content.body.to_lowercase().contains("no active sprint"),
        "body should explicitly say there is no active sprint: {}",
        content.body
    );
    assert!(
        content.body.to_lowercase().contains("backlog"),
        "body should include a backlog note: {}",
        content.body
    );
}

#[tokio::test]
async fn fetch_jira_board_context_groups_sprint_issues_by_board_column() {
    let requester = FakeRequester::new(vec![
        Ok(board_configuration_response()),
        Ok(json!({
            "values": [
                {
                    "id": 100,
                    "name": "Sprint 4",
                    "state": "active",
                    "originBoardId": 12,
                    "goal": "Ship the board chip"
                }
            ],
            "isLast": true
        })),
        Ok(json!({
            "startAt": 0,
            "maxResults": 100,
            "total": 2,
            "issues": [
                {
                    "key": "RX-1",
                    "fields": {
                        "summary": "Add board fetch",
                        "status": { "id": "3", "name": "In Progress" },
                        "assignee": { "displayName": "Ada" }
                    }
                },
                {
                    "key": "RX-2",
                    "fields": {
                        "summary": "Write board tests",
                        "status": { "id": "5", "name": "Done" },
                        "assignee": null
                    }
                }
            ]
        })),
    ]);

    let content = fetch_jira_board_context(&requester, &auth_context(), &board_reference())
        .await
        .expect("board context should resolve");

    assert_eq!(content.title, "Board: RX Board");
    assert!(content.body.contains("Sprint 4"));
    assert!(content.body.contains("Ship the board chip"));
    assert!(content.body.contains("In Progress"));
    assert!(content.body.contains("RX-1: Add board fetch"));
    assert!(content.body.contains("Ada"));
    assert!(content.body.contains("RX-2: Write board tests"));
    assert!(content.body.contains("Unassigned"));

    let in_progress_index = content.body.find("### In Progress").expect("column header");
    let rx1_index = content.body.find("RX-1").expect("issue in body");
    let done_index = content.body.find("### Done").expect("done header");
    let rx2_index = content.body.find("RX-2").expect("issue in body");
    assert!(in_progress_index < rx1_index);
    assert!(rx1_index < done_index);
    assert!(done_index < rx2_index);
}

#[tokio::test]
async fn fetch_jira_board_context_rejects_blank_board_id() {
    let requester = FakeRequester::new(vec![]);
    let mut reference = board_reference();
    reference.id = "   ".to_string();

    let error = fetch_jira_board_context(&requester, &auth_context(), &reference)
        .await
        .unwrap_err();

    assert_eq!(error, "Jira board id is required");
}
