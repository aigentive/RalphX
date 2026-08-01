use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use crate::application::clickup_integration_service::ClickUpTaskListOptions;
use crate::application::{ClickUpApiClient, ClickUpAuthContext};

use super::clickup_client::{
    apply_task_tags, assign_task_to_user, clear_task_assignees, clickup_authorization_header,
    create_task_comment, fetch_current_user, fetch_filtered_tasks, fetch_folder_lists,
    fetch_folder_statuses, fetch_folderless_lists, fetch_list_statuses, fetch_list_tasks,
    fetch_space_folders, fetch_space_statuses, fetch_spaces, fetch_task_detail,
    fetch_task_detail_by_custom_id, fetch_workspaces, map_status_type_to_category, put_task_status,
    validate_token, ClickUpJsonRequester, HyperClickUpApiClient,
};

#[derive(Clone, Debug)]
struct RecordedRequest {
    method: Method,
    url: String,
    token: String,
    body: Option<Value>,
}

#[derive(Default)]
struct FakeClickUpRequester {
    responses: Mutex<VecDeque<Result<Value, String>>>,
    requests: Mutex<Vec<RecordedRequest>>,
}

impl FakeClickUpRequester {
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
impl ClickUpJsonRequester for FakeClickUpRequester {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        self.requests
            .lock()
            .expect("requests")
            .push(RecordedRequest {
                method,
                url,
                token: token.to_string(),
                body,
            });
        self.responses
            .lock()
            .expect("responses")
            .pop_front()
            .unwrap_or_else(|| Err("unexpected ClickUp request".to_string()))
    }
}

fn sample_task(id: &str, status_type: &str) -> Value {
    json!({
        "id": id,
        "name": "Fix login",
        "url": format!("https://app.clickup.com/t/{id}"),
        "status": { "status": "in progress", "type": status_type, "color": "#abc" },
        "assignees": [{ "id": 42, "username": "dev", "email": "dev@example.com" }],
        "followers": [{ "id": 99, "username": "watcher", "email": "watcher@example.com" }],
        "tags": [{ "name": "bug" }, { "name": "backend" }],
        "locations": [
            {
                "id": "sprint-1",
                "name": "Current Sprint",
                "folder": { "id": "folder-1" },
                "space": { "id": "space-location-1" }
            },
            {
                "id": "sprint-2",
                "folder_id": "folder-2",
                "space_id": "space-location-2"
            }
        ],
        "space": { "id": "space-1" },
        "folder": { "id": "folder-primary" },
        "list": { "id": "list-1", "name": "Sprint" },
        "date_updated": "1700000000000"
    })
}

#[test]
fn authorization_header_is_raw_token_without_bearer() {
    let header = clickup_authorization_header("pk_12345_ABCDEF");
    assert_eq!(header, "pk_12345_ABCDEF");
    assert!(!header.to_ascii_lowercase().contains("bearer"));
    assert!(!header.to_ascii_lowercase().contains("basic"));
}

#[test]
fn status_type_maps_to_expected_category() {
    assert_eq!(map_status_type_to_category("open"), "todo");
    assert_eq!(map_status_type_to_category("custom"), "in_progress");
    assert_eq!(map_status_type_to_category("done"), "done");
    assert_eq!(map_status_type_to_category("closed"), "done");
    // Unknown/unexpected types fall back to in_progress.
    assert_eq!(map_status_type_to_category("weird"), "in_progress");
}

#[test]
fn constructing_client_does_not_panic_when_roots_are_unavailable() {
    let result = std::panic::catch_unwind(HyperClickUpApiClient::new);
    assert!(result.is_ok());
}

#[tokio::test]
async fn validate_token_succeeds_on_ok_and_fails_on_error() {
    let ok = FakeClickUpRequester::new(vec![Ok(json!({ "user": { "id": 1 } }))]);
    assert!(validate_token(&ok, "tok").await.is_ok());

    let err = FakeClickUpRequester::new(vec![Err("ClickUp returned HTTP 401".to_string())]);
    assert!(validate_token(&err, "tok").await.is_err());
}

#[tokio::test]
async fn requester_trait_impl_delegates_clickup_api_client_methods() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "user": { "id": 42, "username": "dev" } })),
        Ok(json!({ "teams": [{ "id": "9000", "name": "Workspace" }] })),
        Ok(json!({ "spaces": [{ "id": "space-1", "name": "Engineering" }] })),
        Ok(json!({ "tasks": [sample_task("task-1", "custom")], "last_page": true })),
        Ok(sample_task("task-1", "done")),
        Ok(json!({ "comments": [] })),
        Ok(sample_task("opaque-from-custom", "custom")),
        Ok(json!({ "comments": [] })),
        Ok(json!({ "statuses": [{ "status": "done", "type": "done" }] })),
        Ok(json!({ "statuses": [{ "status": "review", "type": "custom" }] })),
        Ok(json!({ "statuses": [{ "status": "ready", "type": "custom" }] })),
        Ok(json!({ "user": { "id": 42, "username": "dev" } })),
        Ok(json!({ "user": { "id": 42, "username": "dev" } })),
        Ok(json!({})),
        Ok(json!({ "assignees": [{ "id": 42 }] })),
        Ok(json!({})),
        Ok(json!({ "id": "comment-1" })),
        Ok(json!({ "tags": [] })),
        Ok(json!({})),
        Ok(json!({})),
    ]);
    let auth = ClickUpAuthContext {
        api_token: "tok".to_string(),
    };
    let client: &dyn ClickUpApiClient = &fake;

    client.validate(&auth).await.unwrap();
    assert_eq!(client.list_workspaces(&auth).await.unwrap()[0].id, "9000");
    assert_eq!(
        client.list_spaces(&auth, "9000").await.unwrap()[0].id,
        "space-1"
    );
    assert_eq!(
        client
            .list_tasks(
                &auth,
                "9000",
                &["space-1".to_string()],
                ClickUpTaskListOptions::default(),
            )
            .await
            .unwrap()[0]
            .id,
        "task-1"
    );
    assert_eq!(
        client.fetch_task(&auth, "task-1").await.unwrap().id,
        "task-1"
    );
    assert_eq!(
        client
            .fetch_task_by_custom_id(&auth, "9000", "TASK-123")
            .await
            .unwrap()
            .id,
        "opaque-from-custom"
    );
    assert_eq!(
        client.list_statuses(&auth, "space-1").await.unwrap()[0].category,
        "done"
    );
    assert_eq!(
        client
            .list_folder_statuses(&auth, "folder-1")
            .await
            .unwrap()[0]
            .status,
        "review"
    );
    assert_eq!(
        client.list_list_statuses(&auth, "list-1").await.unwrap()[0].status,
        "ready"
    );
    assert_eq!(client.current_user(&auth).await.unwrap().id, 42);
    assert_eq!(
        client
            .assign_task_to_current_user(&auth, "task-1")
            .await
            .unwrap()
            .id,
        42
    );
    client.clear_task_assignee(&auth, "task-1").await.unwrap();
    assert_eq!(
        client
            .create_comment(&auth, "task-1", "done")
            .await
            .unwrap()
            .id,
        "comment-1"
    );
    client
        .set_task_tags(&auth, "task-1", vec!["release".to_string()])
        .await
        .unwrap();

    let requests = fake.requests();
    assert_eq!(requests.len(), 19);
    assert!(requests.iter().all(|request| request.token == "tok"));
}

#[tokio::test]
async fn current_user_maps_user_object() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "user": { "id": 42, "username": "dev", "email": "dev@example.com" }
    }))]);

    let user = fetch_current_user(&fake, "tok").await.unwrap();

    assert_eq!(user.id, 42);
    assert_eq!(user.username.as_deref(), Some("dev"));
    assert_eq!(user.email.as_deref(), Some("dev@example.com"));
    let requests = fake.requests();
    assert_eq!(requests[0].method, Method::GET);
    assert!(requests[0].url.ends_with("/user"));
    assert_eq!(requests[0].token, "tok");
}

#[tokio::test]
async fn current_user_errors_when_user_payload_is_missing() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "ok": true }))]);

    let error = fetch_current_user(&fake, "tok").await.unwrap_err();

    assert_eq!(error, "ClickUp user response was missing user details");
}

#[tokio::test]
async fn workspaces_map_from_teams() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "teams": [
            { "id": "9000", "name": "Acme", "color": "#ff6b35" },
            { "id": "9001", "name": "Beta" }
        ]
    }))]);

    let workspaces = fetch_workspaces(&fake, "tok").await.unwrap();

    assert_eq!(workspaces.len(), 2);
    assert_eq!(workspaces[0].id, "9000");
    assert_eq!(workspaces[0].name, "Acme");
    assert_eq!(workspaces[0].color.as_deref(), Some("#ff6b35"));
    assert_eq!(workspaces[1].id, "9001");
    assert_eq!(workspaces[1].color, None);
}

#[tokio::test]
async fn spaces_map_from_team_spaces() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "spaces": [
            { "id": "space-1", "name": "Engineering", "private": false },
            { "id": "space-2", "name": "Secret", "private": true }
        ]
    }))]);

    let spaces = fetch_spaces(&fake, "tok", "9000").await.unwrap();

    assert_eq!(spaces.len(), 2);
    assert_eq!(spaces[0].id, "space-1");
    assert!(!spaces[0].private);
    assert!(spaces[1].private);
    assert!(fake.requests()[0].url.contains("/team/9000/space"));
}

#[tokio::test]
async fn space_statuses_map_with_category() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "statuses": [
            { "status": "to do", "type": "open", "color": "#aaa", "orderindex": 0 },
            { "status": "in progress", "type": "custom", "orderindex": 1 },
            { "status": "complete", "type": "done", "orderindex": 2 }
        ]
    }))]);

    let statuses = fetch_space_statuses(&fake, "tok", "space-1").await.unwrap();

    assert_eq!(statuses.len(), 3);
    assert_eq!(statuses[0].status, "to do");
    assert_eq!(statuses[0].status_type, "open");
    assert_eq!(statuses[0].category, "todo");
    assert_eq!(statuses[1].category, "in_progress");
    assert_eq!(statuses[2].category, "done");
}

#[tokio::test]
async fn folder_and_list_statuses_map_with_category() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({
            "statuses": [
                { "status": "awaiting deploy", "type": "custom", "color": "#0099aa", "orderindex": 4 }
            ]
        })),
        Ok(json!({
            "statuses": [
                { "status": "done", "type": "closed", "color": "#008844", "orderindex": 9 }
            ]
        })),
    ]);

    let folder_statuses = fetch_folder_statuses(&fake, "tok", "folder-1")
        .await
        .unwrap();
    let list_statuses = fetch_list_statuses(&fake, "tok", "list-1").await.unwrap();

    assert_eq!(folder_statuses[0].status, "awaiting deploy");
    assert_eq!(folder_statuses[0].category, "in_progress");
    assert_eq!(folder_statuses[0].color.as_deref(), Some("#0099aa"));
    assert_eq!(folder_statuses[0].orderindex, Some(4));
    assert_eq!(list_statuses[0].status, "done");
    assert_eq!(list_statuses[0].category, "done");
    assert!(fake.requests()[0].url.contains("/folder/folder-1"));
    assert!(fake.requests()[1].url.contains("/list/list-1"));
}

#[tokio::test]
async fn filtered_tasks_paginate_until_last_page() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "tasks": [sample_task("t1", "open")], "last_page": false })),
        Ok(json!({ "tasks": [sample_task("t2", "custom")], "last_page": true })),
    ]);

    let tasks = fetch_filtered_tasks(
        &fake,
        "tok",
        "9000",
        &["space-1".to_string()],
        ClickUpTaskListOptions::default(),
    )
    .await
    .unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "t1");
    assert_eq!(tasks[1].id, "t2");

    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "should fetch exactly two pages");
    assert!(requests[0].url.contains("page=0"));
    assert!(requests[0].url.contains("order_by=updated"));
    assert!(requests[0].url.contains("reverse=true"));
    assert!(requests[0].url.contains("include_closed=true"));
    assert!(requests[0].url.contains("subtasks=true"));
    assert!(requests[0].url.contains("space-1"));
    assert!(requests[0].url.contains("/team/9000/task"));
    assert!(requests[1].url.contains("page=1"));
    assert_eq!(requests[0].token, "tok");
}

#[tokio::test]
async fn filtered_tasks_stops_on_first_last_page() {
    let fake = FakeClickUpRequester::new(vec![Ok(
        json!({ "tasks": [sample_task("t1", "open")], "last_page": true }),
    )]);

    let tasks = fetch_filtered_tasks(&fake, "tok", "9000", &[], ClickUpTaskListOptions::default())
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(fake.requests().len(), 1, "must not request a second page");
}

#[tokio::test]
async fn filtered_tasks_stops_at_safety_page_cap() {
    let responses = (0..60)
        .map(|page| {
            Ok(json!({
                "tasks": [sample_task(&format!("t{page}"), "custom")],
                "last_page": false
            }))
        })
        .collect();
    let fake = FakeClickUpRequester::new(responses);

    let tasks = fetch_filtered_tasks(&fake, "tok", "9000", &[], ClickUpTaskListOptions::default())
        .await
        .unwrap();

    assert_eq!(tasks.len(), 50);
    assert_eq!(fake.requests().len(), 50);
    assert!(fake.requests()[49].url.contains("page=49"));
}

#[tokio::test]
async fn filtered_tasks_searches_metadata_and_stops_at_limit() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({
            "tasks": [
                sample_task("skip-1", "open"),
                {
                    "id": "match-title",
                    "name": "Alpha demo task",
                    "status": { "status": "to do", "type": "open" },
                    "assignees": [{ "username": "Alex" }],
                    "tags": []
                }
            ],
            "last_page": false
        })),
        Ok(json!({
            "tasks": [
                {
                    "id": "match-tag",
                    "name": "Other task",
                    "status": { "status": "to do", "type": "open" },
                    "assignees": [],
                    "tags": [{ "name": "Alpha" }]
                }
            ],
            "last_page": false
        })),
    ]);

    let tasks = fetch_filtered_tasks(
        &fake,
        "tok",
        "9000",
        &[],
        ClickUpTaskListOptions {
            query: Some("alpha".to_string()),
            limit: Some(2),
            assignee_ids: Vec::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(
        tasks
            .iter()
            .map(|task| task.id.as_str())
            .collect::<Vec<_>>(),
        vec!["match-title", "match-tag"]
    );
    assert_eq!(
        fake.requests().len(),
        2,
        "search should stop once enough matching tasks are found"
    );
}

#[tokio::test]
async fn filtered_tasks_matches_key_status_list_and_assignee_metadata() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "tasks": [
            {
                "id": "opaque-1",
                "custom_id": "TASK-123",
                "name": "Different title",
                "status": { "status": "Awaiting Staging", "type": "custom" },
                "assignees": [{ "email": "alex@example.com" }],
                "tags": [],
                "list": { "name": "Current Sprint" }
            },
            {
                "id": "opaque-2",
                "name": "Another title",
                "status": { "status": "to do", "type": "open" },
                "assignees": [{ "username": "Alex" }],
                "tags": [],
                "list": { "name": "Backlog" }
            }
        ],
        "last_page": true
    }))]);

    let by_key = fetch_filtered_tasks(
        &fake,
        "tok",
        "9000",
        &[],
        ClickUpTaskListOptions {
            query: Some("task-123".to_string()),
            limit: Some(10),
            assignee_ids: Vec::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(by_key.len(), 1);
    assert_eq!(by_key[0].id, "opaque-1");
    assert_eq!(by_key[0].list_name.as_deref(), Some("Current Sprint"));
}

#[tokio::test]
async fn filtered_tasks_matches_status_list_and_assignee_queries() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({
            "tasks": [
                {
                    "id": "status-match",
                    "name": "Unrelated title",
                    "status": { "status": "Awaiting Staging", "type": "custom" },
                    "assignees": [],
                    "tags": [],
                    "list": { "name": "Backlog" }
                }
            ],
            "last_page": true
        })),
        Ok(json!({
            "tasks": [
                {
                    "id": "list-match",
                    "name": "Unrelated title",
                    "status": { "status": "to do", "type": "open" },
                    "assignees": [],
                    "tags": [],
                    "list": { "name": "Current Sprint" }
                }
            ],
            "last_page": true
        })),
        Ok(json!({
            "tasks": [
                {
                    "id": "assignee-match",
                    "name": "Unrelated title",
                    "status": { "status": "to do", "type": "open" },
                    "assignees": [{ "email": "alex@example.com" }],
                    "tags": [],
                    "list": { "name": "Backlog" }
                }
            ],
            "last_page": true
        })),
    ]);

    for (query, expected_id) in [
        ("awaiting staging", "status-match"),
        ("current sprint", "list-match"),
        ("alex@example.com", "assignee-match"),
    ] {
        let tasks = fetch_filtered_tasks(
            &fake,
            "tok",
            "9000",
            &[],
            ClickUpTaskListOptions {
                query: Some(query.to_string()),
                limit: Some(10),
                assignee_ids: Vec::new(),
            },
        )
        .await
        .unwrap();

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, expected_id);
    }
}

#[tokio::test]
async fn filtered_tasks_stops_inside_page_when_limit_is_reached() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({
        "tasks": [
            sample_task("match-1", "custom"),
            sample_task("match-2", "custom")
        ],
        "last_page": false
    }))]);

    let tasks = fetch_filtered_tasks(
        &fake,
        "tok",
        "9000",
        &[],
        ClickUpTaskListOptions {
            query: Some("fix".to_string()),
            limit: Some(1),
            assignee_ids: Vec::new(),
        },
    )
    .await
    .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "match-1");
    assert_eq!(
        fake.requests().len(),
        1,
        "limit should stop before fetching a second page"
    );
}

#[tokio::test]
async fn filtered_tasks_encodes_workspace_and_space_ids() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "tasks": [], "last_page": true }))]);

    fetch_filtered_tasks(
        &fake,
        "tok",
        "team/with space",
        &["space/one".to_string(), "space two".to_string()],
        ClickUpTaskListOptions {
            assignee_ids: vec![42, 99],
            ..ClickUpTaskListOptions::default()
        },
    )
    .await
    .unwrap();

    let url = &fake.requests()[0].url;
    assert!(url.contains("/team/team%2Fwith%20space/task"));
    assert!(url.contains("space_ids%5B%5D=space%2Fone"));
    assert!(url.contains("space_ids%5B%5D=space%20two"));
    assert!(url.contains("assignees%5B%5D=42"));
    assert!(url.contains("assignees%5B%5D=99"));
}

#[tokio::test]
async fn list_tasks_uses_list_endpoint_and_assignee_filter() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "tasks": [], "last_page": true }))]);

    fetch_list_tasks(
        &fake,
        "tok",
        "list/one",
        ClickUpTaskListOptions {
            assignee_ids: vec![42],
            ..ClickUpTaskListOptions::default()
        },
    )
    .await
    .unwrap();

    let url = &fake.requests()[0].url;
    assert!(url.contains("/list/list%2Fone/task"));
    assert!(url.contains("order_by=updated"));
    assert!(url.contains("reverse=true"));
    assert!(url.contains("include_closed=true"));
    assert!(url.contains("subtasks=true"));
    assert!(url.contains("assignees%5B%5D=42"));
}

#[tokio::test]
async fn folder_and_list_hierarchy_requests_map_parent_fallbacks() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({
            "folders": [
                { "id": "folder-1", "name": "Platform", "space": { "id": "space-body" } },
                { "id": "folder-2", "name": "Fallback Space" },
                { "name": "missing id" }
            ]
        })),
        Ok(json!({
            "lists": [
                {
                    "id": "list-1",
                    "name": "Folder List",
                    "folder": { "id": "folder-body" },
                    "space": { "id": "space-1" }
                },
                { "id": "list-2", "name": "Fallback Folder" }
            ]
        })),
        Ok(json!({
            "lists": [
                { "id": "list-3", "name": "Space List", "space_id": "space-flat" },
                { "id": "list-4", "name": "Fallback Space" }
            ]
        })),
    ]);

    let folders = fetch_space_folders(&fake, "tok", "space/one")
        .await
        .unwrap();
    let folder_lists = fetch_folder_lists(&fake, "tok", "folder/one")
        .await
        .unwrap();
    let folderless_lists = fetch_folderless_lists(&fake, "tok", "space/one")
        .await
        .unwrap();

    assert_eq!(folders.len(), 2);
    assert_eq!(folders[0].id, "folder-1");
    assert_eq!(folders[0].space_id.as_deref(), Some("space-body"));
    assert_eq!(folders[1].space_id.as_deref(), Some("space/one"));
    assert_eq!(folder_lists.len(), 2);
    assert_eq!(folder_lists[0].folder_id.as_deref(), Some("folder-body"));
    assert_eq!(folder_lists[1].folder_id.as_deref(), Some("folder/one"));
    assert_eq!(folderless_lists.len(), 2);
    assert_eq!(folderless_lists[0].space_id.as_deref(), Some("space-flat"));
    assert_eq!(folderless_lists[1].space_id.as_deref(), Some("space/one"));

    let requests = fake.requests();
    assert!(requests[0]
        .url
        .ends_with("/space/space%2Fone/folder?archived=false"));
    assert!(requests[1]
        .url
        .ends_with("/folder/folder%2Fone/list?archived=false"));
    assert!(requests[2]
        .url
        .ends_with("/space/space%2Fone/list?archived=false"));
}

#[tokio::test]
async fn requester_trait_impl_delegates_clickup_hierarchy_methods() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "folders": [{ "id": "folder-1", "name": "Platform" }] })),
        Ok(json!({ "lists": [{ "id": "list-1", "name": "Folder List" }] })),
        Ok(json!({ "lists": [{ "id": "list-2", "name": "Space List" }] })),
        Ok(json!({ "tasks": [sample_task("list-task", "custom")], "last_page": true })),
    ]);
    let auth = ClickUpAuthContext {
        api_token: "tok".to_string(),
    };
    let client: &dyn ClickUpApiClient = &fake;

    assert_eq!(
        client.list_folders(&auth, "space-1").await.unwrap()[0].id,
        "folder-1"
    );
    assert_eq!(
        client.list_folder_lists(&auth, "folder-1").await.unwrap()[0].id,
        "list-1"
    );
    assert_eq!(
        client
            .list_folderless_lists(&auth, "space-1")
            .await
            .unwrap()[0]
            .id,
        "list-2"
    );
    assert_eq!(
        client
            .list_tasks_for_list(&auth, "list-1", ClickUpTaskListOptions::default())
            .await
            .unwrap()[0]
            .id,
        "list-task"
    );
}

#[tokio::test]
async fn task_summary_maps_status_assignees_and_tags() {
    let fake = FakeClickUpRequester::new(vec![Ok(
        json!({ "tasks": [sample_task("abc123", "custom")], "last_page": true }),
    )]);

    let tasks = fetch_filtered_tasks(
        &fake,
        "tok",
        "9000",
        &["space-1".to_string()],
        ClickUpTaskListOptions::default(),
    )
    .await
    .unwrap();

    let task = &tasks[0];
    assert_eq!(task.id, "abc123");
    assert_eq!(task.name, "Fix login");
    assert_eq!(task.status_name.as_deref(), Some("in progress"));
    assert_eq!(task.status_type.as_deref(), Some("custom"));
    assert_eq!(task.status_category.as_deref(), Some("in_progress"));
    assert_eq!(task.assignees, vec!["dev".to_string()]);
    assert_eq!(task.assignee_ids, vec![42]);
    assert_eq!(task.watchers.len(), 1);
    assert_eq!(task.watchers[0].id, 99);
    assert_eq!(task.watchers[0].username.as_deref(), Some("watcher"));
    assert_eq!(
        task.watchers[0].email.as_deref(),
        Some("watcher@example.com")
    );
    assert_eq!(task.tags, vec!["bug".to_string(), "backend".to_string()]);
    assert_eq!(task.sprint_names, vec!["Current Sprint".to_string()]);
    assert_eq!(
        task.location_ids,
        vec!["sprint-1".to_string(), "sprint-2".to_string()]
    );
    assert_eq!(
        task.location_folder_ids,
        vec!["folder-1".to_string(), "folder-2".to_string()]
    );
    assert_eq!(
        task.location_space_ids,
        vec![
            "space-location-1".to_string(),
            "space-location-2".to_string()
        ]
    );
    assert_eq!(task.space_id.as_deref(), Some("space-1"));
    assert_eq!(task.folder_id.as_deref(), Some("folder-primary"));
    assert_eq!(task.list_id.as_deref(), Some("list-1"));
    assert_eq!(task.list_name.as_deref(), Some("Sprint"));
    assert_eq!(
        task.updated_at.as_deref(),
        Some("2023-11-14T22:13:20+00:00")
    );
}

#[tokio::test]
async fn fetch_task_detail_maps_fields() {
    let mut task = sample_task("abc123", "done");
    task["description"] = json!("Detailed body");
    task["creator"] = json!({ "id": 1, "username": "owner" });
    task["attachments"] = json!([
        {
            "id": "att-1",
            "filename": "mockup.png",
            "mime_type": "image/png",
            "size": 2048,
            "url": "https://files.example/mockup.png"
        }
    ]);
    let fake = FakeClickUpRequester::new(vec![
        Ok(task),
        Ok(json!({
            "comments": [
                {
                    "id": 12345,
                    "comment_text": "Looks loaded",
                    "user": { "id": 7, "username": "Reviewer" },
                    "date": "1700000000000",
                    "reply_count": 1,
                    "attachments": [
                        {
                            "id": "comment-att-1",
                            "filename": "comment-shot.jpg",
                            "mime_type": "image/jpeg",
                            "size": 1024,
                            "url": "https://files.example/comment-shot.jpg"
                        }
                    ]
                },
                {
                    "id": "fragmented",
                    "comment": [{ "text": "Fragment " }, { "text": "body" }],
                    "user": { "email": "reviewer@example.com" }
                }
            ]
        })),
        Ok(json!({
            "comments": [
                {
                    "id": "reply-1",
                    "comment_text": "Thread reply",
                    "user": { "username": "Responder" },
                    "date": "1700000001000"
                }
            ]
        })),
    ]);

    let content = fetch_task_detail(&fake, "tok", "abc123").await.unwrap();

    assert_eq!(content.id, "abc123");
    assert_eq!(content.description, "Detailed body");
    assert_eq!(content.status_category.as_deref(), Some("done"));
    assert_eq!(content.creator.as_deref(), Some("owner"));
    assert_eq!(content.assignees, vec!["dev".to_string()]);
    assert_eq!(content.watchers.len(), 1);
    assert_eq!(content.watchers[0].id, 99);
    assert_eq!(content.attachments.len(), 1);
    assert_eq!(content.attachments[0].filename, "mockup.png");
    assert_eq!(
        content.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(
        content.attachments[0].url.as_deref(),
        Some("https://files.example/mockup.png")
    );
    assert_eq!(content.comments.len(), 2);
    assert_eq!(content.comments[0].id, "12345");
    assert_eq!(content.comments[0].body, "Looks loaded");
    assert_eq!(content.comments[0].author_name.as_deref(), Some("Reviewer"));
    assert_eq!(content.comments[0].attachments.len(), 1);
    assert_eq!(
        content.comments[0].attachments[0].filename,
        "comment-shot.jpg"
    );
    assert_eq!(
        content.comments[0].created_at.as_deref(),
        Some("2023-11-14T22:13:20+00:00"),
    );
    assert_eq!(content.comments[0].replies.len(), 1);
    assert_eq!(content.comments[0].replies[0].body, "Thread reply");
    assert_eq!(
        content.comments[0].replies[0].created_at.as_deref(),
        Some("2023-11-14T22:13:21+00:00"),
    );
    assert_eq!(
        content.comments[0].replies[0].author_name.as_deref(),
        Some("Responder"),
    );
    assert_eq!(content.comments[1].body, "Fragment body");
    assert_eq!(
        content.comments[1].author_name.as_deref(),
        Some("reviewer@example.com"),
    );
    assert!(fake.requests()[0].url.ends_with("/task/abc123"));
    assert!(fake.requests()[1].url.ends_with("/task/abc123/comment"));
    assert!(fake.requests()[2].url.ends_with("/comment/12345/reply"));
}

#[tokio::test]
async fn fetch_task_detail_by_custom_id_adds_team_query_to_task_and_comments() {
    let mut task = sample_task("opaque-1", "custom");
    task["custom_id"] = json!("TASK-123");
    let fake = FakeClickUpRequester::new(vec![
        Ok(task),
        Ok(json!({
            "comments": [
                {
                    "id": "comment-1",
                    "comment_text": "Custom id comment",
                    "user": { "username": "Reviewer" }
                }
            ]
        })),
    ]);

    let content = fetch_task_detail_by_custom_id(&fake, "tok", "workspace-1", "TASK-123")
        .await
        .unwrap();

    assert_eq!(content.id, "opaque-1");
    assert_eq!(content.custom_id.as_deref(), Some("TASK-123"));
    assert_eq!(content.comments[0].body, "Custom id comment");
    assert!(fake.requests()[0]
        .url
        .ends_with("/task/TASK-123?custom_task_ids=true&team_id=workspace-1"));
    assert!(fake.requests()[1]
        .url
        .ends_with("/task/TASK-123/comment?custom_task_ids=true&team_id=workspace-1"));
}

#[tokio::test]
async fn fetch_task_detail_maps_clickup_attachment_fallback_fields() {
    let mut task = sample_task("abc123", "custom");
    task["date_updated"] = json!("2026-06-23T12:00:00Z");
    task["attachments"] = json!([
        {
            "uuid": "uuid-1",
            "title": "design-shot.png",
            "mimeType": "image/png",
            "file_size": 4096,
            "downloadUrl": "https://files.example/design-shot.png"
        },
        {
            "name": "brief.txt",
            "content_type": "text/plain",
            "download_url": "https://files.example/brief.txt"
        }
    ]);
    let fake = FakeClickUpRequester::new(vec![
        Ok(task),
        Ok(json!({
            "comments": [
                {
                    "id": "comment-1",
                    "body": "Body fallback",
                    "user": { "name": "Named User" },
                    "date": "2026-06-23T12:05:00Z"
                }
            ]
        })),
    ]);

    let content = fetch_task_detail(&fake, "tok", "abc123").await.unwrap();

    assert_eq!(content.updated_at.as_deref(), Some("2026-06-23T12:00:00Z"));
    assert_eq!(content.attachments.len(), 2);
    assert_eq!(content.attachments[0].id.as_deref(), Some("uuid-1"));
    assert_eq!(content.attachments[0].filename, "design-shot.png");
    assert_eq!(
        content.attachments[0].mime_type.as_deref(),
        Some("image/png")
    );
    assert_eq!(content.attachments[0].size, Some(4096));
    assert_eq!(
        content.attachments[0].url.as_deref(),
        Some("https://files.example/design-shot.png"),
    );
    assert_eq!(content.attachments[1].filename, "brief.txt");
    assert_eq!(
        content.attachments[1].mime_type.as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        content.attachments[1].url.as_deref(),
        Some("https://files.example/brief.txt"),
    );
    assert_eq!(content.comments[0].body, "Body fallback");
    assert_eq!(
        content.comments[0].author_name.as_deref(),
        Some("Named User")
    );
    assert_eq!(
        content.comments[0].created_at.as_deref(),
        Some("2026-06-23T12:05:00Z"),
    );
    assert_eq!(
        fake.requests().len(),
        2,
        "comment had no replies to hydrate"
    );
}

#[tokio::test]
async fn fetch_task_detail_errors_when_task_payload_is_missing() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "name": "Missing id" }))]);

    let error = fetch_task_detail(&fake, "tok", "abc123").await.unwrap_err();

    assert_eq!(error, "ClickUp task response was missing task details");
    assert_eq!(
        fake.requests().len(),
        1,
        "comments must not load when the task payload is invalid"
    );
}

#[tokio::test]
async fn put_task_status_sends_status_body() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({}))]);

    put_task_status(&fake, "tok", "abc123", "in progress")
        .await
        .unwrap();

    let request = &fake.requests()[0];
    assert_eq!(request.method, Method::PUT);
    assert!(request.url.ends_with("/task/abc123"));
    assert_eq!(request.body, Some(json!({ "status": "in progress" })));
}

#[tokio::test]
async fn create_comment_posts_comment_text() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "id": 12345, "date": "1700000000000" }))]);

    let comment = create_task_comment(&fake, "tok", "abc123", "looks good")
        .await
        .unwrap();

    assert_eq!(comment.id, "12345");
    assert_eq!(comment.body, "looks good");
    let request = &fake.requests()[0];
    assert_eq!(request.method, Method::POST);
    assert!(request.url.ends_with("/task/abc123/comment"));
    assert_eq!(request.body, Some(json!({ "comment_text": "looks good" })));
}

#[tokio::test]
async fn assign_task_resolves_current_user_then_adds_assignee() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "user": { "id": 42, "username": "dev" } })),
        Ok(json!({})),
    ]);

    let user = assign_task_to_user(&fake, "tok", "abc123").await.unwrap();

    assert_eq!(user.id, 42);
    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, Method::GET);
    assert!(requests[0].url.ends_with("/user"));
    assert_eq!(requests[1].method, Method::PUT);
    assert_eq!(
        requests[1].body,
        Some(json!({ "assignees": { "add": [42] } }))
    );
}

#[tokio::test]
async fn clear_assignees_removes_existing_assignees() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "assignees": [{ "id": 42 }, { "id": 7 }] })),
        Ok(json!({})),
    ]);

    clear_task_assignees(&fake, "tok", "abc123").await.unwrap();

    let requests = fake.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].method, Method::PUT);
    assert_eq!(
        requests[1].body,
        Some(json!({ "assignees": { "rem": [42, 7] } }))
    );
}

#[tokio::test]
async fn clear_assignees_noop_when_already_unassigned() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "assignees": [] }))]);

    clear_task_assignees(&fake, "tok", "abc123").await.unwrap();

    // Only the GET; no PUT issued when there is nothing to remove.
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn apply_tags_adds_missing_and_removes_extra() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "tags": [{ "name": "old" }, { "name": "keep" }] })),
        Ok(json!({})), // DELETE old
        Ok(json!({})), // POST new
    ]);

    apply_task_tags(
        &fake,
        "tok",
        "abc123",
        vec!["keep".to_string(), "new".to_string()],
    )
    .await
    .unwrap();

    let requests = fake.requests();
    assert_eq!(requests[0].method, Method::GET);
    assert_eq!(requests[1].method, Method::DELETE);
    assert!(requests[1].url.ends_with("/task/abc123/tag/old"));
    assert_eq!(requests[2].method, Method::POST);
    assert!(requests[2].url.ends_with("/task/abc123/tag/new"));
}

#[tokio::test]
async fn apply_tags_ignores_case_matches_and_blank_desired_tags() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "tags": [{ "name": "Keep" }] }))]);

    apply_task_tags(
        &fake,
        "tok",
        "abc123",
        vec!["keep".to_string(), "   ".to_string()],
    )
    .await
    .unwrap();

    assert_eq!(
        fake.requests().len(),
        1,
        "case-insensitive matches and blanks need no mutation calls"
    );
}
