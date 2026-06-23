use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use hyper::Method;
use serde_json::{json, Value};

use super::clickup_client::{
    apply_task_tags, assign_task_to_user, clear_task_assignees, clickup_authorization_header,
    create_task_comment, fetch_current_user, fetch_filtered_tasks, fetch_space_statuses,
    fetch_spaces, fetch_task_detail, fetch_workspaces, map_status_type_to_category,
    put_task_status, validate_token, ClickUpJsonRequester, HyperClickUpApiClient,
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
        "tags": [{ "name": "bug" }, { "name": "backend" }],
        "space": { "id": "space-1" },
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
async fn filtered_tasks_paginate_until_last_page() {
    let fake = FakeClickUpRequester::new(vec![
        Ok(json!({ "tasks": [sample_task("t1", "open")], "last_page": false })),
        Ok(json!({ "tasks": [sample_task("t2", "custom")], "last_page": true })),
    ]);

    let tasks = fetch_filtered_tasks(&fake, "tok", "9000", &["space-1".to_string()])
        .await
        .unwrap();

    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, "t1");
    assert_eq!(tasks[1].id, "t2");

    let requests = fake.requests();
    assert_eq!(requests.len(), 2, "should fetch exactly two pages");
    assert!(requests[0].url.contains("page=0"));
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

    let tasks = fetch_filtered_tasks(&fake, "tok", "9000", &[])
        .await
        .unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(fake.requests().len(), 1, "must not request a second page");
}

#[tokio::test]
async fn filtered_tasks_encodes_workspace_and_space_ids() {
    let fake = FakeClickUpRequester::new(vec![Ok(json!({ "tasks": [], "last_page": true }))]);

    fetch_filtered_tasks(
        &fake,
        "tok",
        "team/with space",
        &["space/one".to_string(), "space two".to_string()],
    )
    .await
    .unwrap();

    let url = &fake.requests()[0].url;
    assert!(url.contains("/team/team%2Fwith%20space/task"));
    assert!(url.contains("space_ids%5B%5D=space%2Fone"));
    assert!(url.contains("space_ids%5B%5D=space%20two"));
}

#[tokio::test]
async fn task_summary_maps_status_assignees_and_tags() {
    let fake = FakeClickUpRequester::new(vec![Ok(
        json!({ "tasks": [sample_task("abc123", "custom")], "last_page": true }),
    )]);

    let tasks = fetch_filtered_tasks(&fake, "tok", "9000", &["space-1".to_string()])
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
    assert_eq!(task.tags, vec!["bug".to_string(), "backend".to_string()]);
    assert_eq!(task.space_id.as_deref(), Some("space-1"));
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
