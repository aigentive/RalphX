use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::{json, Value};
use tokio::time::Duration;
use tokio_util::bytes::Bytes;

use crate::domain::integrations::{
    ClickUpApiClient, ClickUpAttachment, ClickUpAuthContext, ClickUpComment, ClickUpFolder,
    ClickUpList, ClickUpSpace, ClickUpStatus, ClickUpTaskContent, ClickUpTaskListOptions,
    ClickUpTaskSummary, ClickUpUser, ClickUpWorkspace,
};

const CLICKUP_API_BASE: &str = "https://api.clickup.com/api/v2";
/// Safety cap so a misbehaving `last_page` flag can never spin forever.
const MAX_TASK_PAGES: usize = 50;

pub struct HyperClickUpApiClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    timeout: Duration,
}

impl HyperClickUpApiClient {
    pub fn new() -> Result<Self, String> {
        install_rustls_crypto_provider();
        let https = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .map_err(|error| format!("native root certificates unavailable: {error}"))?
            .https_only()
            .enable_http1()
            .build();
        Ok(Self {
            client: Client::builder(TokioExecutor::new()).build(https),
            timeout: Duration::from_secs(20),
        })
    }

    async fn send_json_request(
        &self,
        method: Method,
        url: String,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let uri = url
            .parse::<hyper::Uri>()
            .map_err(|error| format!("Invalid ClickUp URL: {error}"))?;
        let body_bytes = body
            .map(|value| serde_json::to_vec(&value))
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_default();
        // ClickUp Personal API tokens are sent verbatim — no `Bearer` prefix.
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Accept", "application/json")
            .header("Authorization", clickup_authorization_header(token));
        if !body_bytes.is_empty() {
            builder = builder.header("Content-Type", "application/json");
        }
        let request = builder
            .body(Full::new(Bytes::from(body_bytes)))
            .map_err(|error| format!("Failed to build ClickUp request: {error}"))?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| "ClickUp request timed out".to_string())?
            .map_err(|error| format!("ClickUp request failed: {error}"))?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| format!("Failed to read ClickUp response: {error}"))?
            .to_bytes();
        if !status.is_success() {
            return Err(format!("ClickUp returned HTTP {}", status.as_u16()));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse ClickUp response: {error}"))
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// The ClickUp `Authorization` header value is the raw Personal API token with
/// NO `Bearer` prefix and NO `Basic` encoding (unlike Atlassian).
pub(crate) fn clickup_authorization_header(token: &str) -> String {
    token.to_string()
}

#[async_trait]
pub(crate) trait ClickUpJsonRequester: Send + Sync {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value, String>;
}

#[async_trait]
impl ClickUpJsonRequester for HyperClickUpApiClient {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        token: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        self.send_json_request(method, url, token, body).await
    }
}

#[async_trait]
impl<T> ClickUpApiClient for T
where
    T: ClickUpJsonRequester + Send + Sync,
{
    async fn validate(&self, auth: &ClickUpAuthContext) -> Result<(), String> {
        validate_token(self, &auth.api_token).await
    }

    async fn list_workspaces(
        &self,
        auth: &ClickUpAuthContext,
    ) -> Result<Vec<ClickUpWorkspace>, String> {
        fetch_workspaces(self, &auth.api_token).await
    }

    async fn list_spaces(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
    ) -> Result<Vec<ClickUpSpace>, String> {
        fetch_spaces(self, &auth.api_token, team_id).await
    }

    async fn list_folders(
        &self,
        auth: &ClickUpAuthContext,
        space_id: &str,
    ) -> Result<Vec<ClickUpFolder>, String> {
        fetch_space_folders(self, &auth.api_token, space_id).await
    }

    async fn list_folder_lists(
        &self,
        auth: &ClickUpAuthContext,
        folder_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        fetch_folder_lists(self, &auth.api_token, folder_id).await
    }

    async fn list_folderless_lists(
        &self,
        auth: &ClickUpAuthContext,
        space_id: &str,
    ) -> Result<Vec<ClickUpList>, String> {
        fetch_folderless_lists(self, &auth.api_token, space_id).await
    }

    async fn list_tasks(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
        space_ids: &[String],
        options: ClickUpTaskListOptions,
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        fetch_filtered_tasks(self, &auth.api_token, team_id, space_ids, options).await
    }

    async fn list_tasks_for_list(
        &self,
        auth: &ClickUpAuthContext,
        list_id: &str,
        options: ClickUpTaskListOptions,
    ) -> Result<Vec<ClickUpTaskSummary>, String> {
        fetch_list_tasks(self, &auth.api_token, list_id, options).await
    }

    async fn fetch_task(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        fetch_task_detail(self, &auth.api_token, task_id).await
    }

    async fn fetch_task_by_custom_id(
        &self,
        auth: &ClickUpAuthContext,
        team_id: &str,
        task_id: &str,
    ) -> Result<ClickUpTaskContent, String> {
        fetch_task_detail_by_custom_id(self, &auth.api_token, team_id, task_id).await
    }

    async fn list_statuses(
        &self,
        auth: &ClickUpAuthContext,
        space_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        fetch_space_statuses(self, &auth.api_token, space_id).await
    }

    async fn list_folder_statuses(
        &self,
        auth: &ClickUpAuthContext,
        folder_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        fetch_folder_statuses(self, &auth.api_token, folder_id).await
    }

    async fn list_list_statuses(
        &self,
        auth: &ClickUpAuthContext,
        list_id: &str,
    ) -> Result<Vec<ClickUpStatus>, String> {
        fetch_list_statuses(self, &auth.api_token, list_id).await
    }

    async fn current_user(&self, auth: &ClickUpAuthContext) -> Result<ClickUpUser, String> {
        fetch_current_user(self, &auth.api_token).await
    }

    async fn update_task_status(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
        status_name: &str,
    ) -> Result<(), String> {
        put_task_status(self, &auth.api_token, task_id, status_name).await
    }

    async fn assign_task_to_current_user(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<ClickUpUser, String> {
        assign_task_to_user(self, &auth.api_token, task_id).await
    }

    async fn clear_task_assignee(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
    ) -> Result<(), String> {
        clear_task_assignees(self, &auth.api_token, task_id).await
    }

    async fn create_comment(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
        body_markdown: &str,
    ) -> Result<ClickUpComment, String> {
        create_task_comment(self, &auth.api_token, task_id, body_markdown).await
    }

    async fn set_task_tags(
        &self,
        auth: &ClickUpAuthContext,
        task_id: &str,
        tags: Vec<String>,
    ) -> Result<(), String> {
        apply_task_tags(self, &auth.api_token, task_id, tags).await
    }
}

// ---------------------------------------------------------------------------
// Mappers — unit-testable via a `ClickUpJsonRequester` fake (no real network).
// ---------------------------------------------------------------------------

/// Maps a ClickUp `status.type` to a RalphX ticketing category.
///
/// ClickUp emits four status types: `open` (not started), `custom`
/// (user-defined active states), `done` (completed), and `closed` (archived).
/// Anything unexpected falls back to `in_progress`.
pub(crate) fn map_status_type_to_category(status_type: &str) -> String {
    match status_type {
        "open" => "todo",
        "custom" => "in_progress",
        "done" | "closed" => "done",
        _ => "in_progress",
    }
    .to_string()
}

pub(crate) async fn validate_token<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
) -> Result<(), String> {
    client
        .request_json(Method::GET, format!("{CLICKUP_API_BASE}/user"), token, None)
        .await
        .map(|_| ())
}

pub(crate) async fn fetch_current_user<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
) -> Result<ClickUpUser, String> {
    let value = client
        .request_json(Method::GET, format!("{CLICKUP_API_BASE}/user"), token, None)
        .await?;
    value
        .get("user")
        .and_then(user_from_value)
        .ok_or_else(|| "ClickUp user response was missing user details".to_string())
}

pub(crate) async fn fetch_workspaces<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
) -> Result<Vec<ClickUpWorkspace>, String> {
    let value = client
        .request_json(Method::GET, format!("{CLICKUP_API_BASE}/team"), token, None)
        .await?;
    Ok(value
        .get("teams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(workspace_from_value)
        .collect())
}

pub(crate) async fn fetch_spaces<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    team_id: &str,
) -> Result<Vec<ClickUpSpace>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/team/{}/space?archived=false",
        percent_encode_path_segment(team_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(value
        .get("spaces")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(space_from_value)
        .collect())
}

pub(crate) async fn fetch_space_folders<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    space_id: &str,
) -> Result<Vec<ClickUpFolder>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/space/{}/folder?archived=false",
        percent_encode_path_segment(space_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(value
        .get("folders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|folder| folder_from_value(folder, Some(space_id)))
        .collect())
}

pub(crate) async fn fetch_folder_lists<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    folder_id: &str,
) -> Result<Vec<ClickUpList>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/folder/{}/list?archived=false",
        percent_encode_path_segment(folder_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(value
        .get("lists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|list| list_from_value(list, Some(folder_id), None))
        .collect())
}

pub(crate) async fn fetch_folderless_lists<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    space_id: &str,
) -> Result<Vec<ClickUpList>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/space/{}/list?archived=false",
        percent_encode_path_segment(space_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(value
        .get("lists")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|list| list_from_value(list, None, Some(space_id)))
        .collect())
}

/// Loads workspace-scoped filtered tasks, walking `?page=N` until ClickUp
/// reports `last_page` (or returns an empty page).
pub(crate) async fn fetch_filtered_tasks<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    team_id: &str,
    space_ids: &[String],
    options: ClickUpTaskListOptions,
) -> Result<Vec<ClickUpTaskSummary>, String> {
    fetch_tasks_by_page(client, token, options, |page, assignee_ids| {
        filtered_tasks_url(team_id, space_ids, assignee_ids, page)
    })
    .await
}

pub(crate) async fn fetch_list_tasks<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    list_id: &str,
    options: ClickUpTaskListOptions,
) -> Result<Vec<ClickUpTaskSummary>, String> {
    fetch_tasks_by_page(client, token, options, |page, assignee_ids| {
        list_tasks_url(list_id, assignee_ids, page)
    })
    .await
}

async fn fetch_tasks_by_page<C, F>(
    client: &C,
    token: &str,
    options: ClickUpTaskListOptions,
    url_for_page: F,
) -> Result<Vec<ClickUpTaskSummary>, String>
where
    C: ClickUpJsonRequester + ?Sized,
    F: Fn(usize, &[i64]) -> String,
{
    let query = options
        .query
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase);
    let limit = options.limit.filter(|limit| *limit > 0);
    let mut tasks = Vec::new();
    let mut page = 0usize;
    loop {
        let url = url_for_page(page, &options.assignee_ids);
        let value = client.request_json(Method::GET, url, token, None).await?;
        let page_tasks = value.get("tasks").and_then(Value::as_array);
        let count = page_tasks.map(Vec::len).unwrap_or(0);
        if let Some(page_tasks) = page_tasks {
            for task in page_tasks {
                if let Some(summary) = task_summary_from_value(task) {
                    if query
                        .as_deref()
                        .is_some_and(|query| !task_summary_matches_query(&summary, query))
                    {
                        continue;
                    }
                    tasks.push(summary);
                    if limit.is_some_and(|limit| tasks.len() >= limit) {
                        break;
                    }
                }
            }
        }
        let last_page = value
            .get("last_page")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        page += 1;
        if limit.is_some_and(|limit| tasks.len() >= limit)
            || last_page
            || count == 0
            || page >= MAX_TASK_PAGES
        {
            break;
        }
    }
    Ok(tasks)
}

pub(crate) async fn fetch_task_detail<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
) -> Result<ClickUpTaskContent, String> {
    fetch_task_detail_with_query(client, token, task_id, None).await
}

pub(crate) async fn fetch_task_detail_by_custom_id<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    team_id: &str,
    task_id: &str,
) -> Result<ClickUpTaskContent, String> {
    let query = clickup_custom_task_id_query(team_id);
    fetch_task_detail_with_query(client, token, task_id, Some(query.as_str())).await
}

async fn fetch_task_detail_with_query<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
    custom_task_query: Option<&str>,
) -> Result<ClickUpTaskContent, String> {
    let task_url = clickup_task_url(task_id, custom_task_query);
    let value = client
        .request_json(Method::GET, task_url, token, None)
        .await?;
    let mut content = task_content_from_value(&value)
        .ok_or_else(|| "ClickUp task response was missing task details".to_string())?;
    let comments_url = clickup_task_comments_url(task_id, custom_task_query);
    let comments = client
        .request_json(Method::GET, comments_url, token, None)
        .await?;
    content.comments = clickup_comments_from_value(&comments);
    for comment in &mut content.comments {
        if clickup_comment_reply_count(&comments, &comment.id) == 0 {
            continue;
        }
        comment.replies = fetch_clickup_comment_replies(client, token, &comment.id).await?;
    }
    Ok(content)
}

fn clickup_task_url(task_id: &str, custom_task_query: Option<&str>) -> String {
    append_query(
        format!(
            "{CLICKUP_API_BASE}/task/{}",
            percent_encode_path_segment(task_id)
        ),
        custom_task_query,
    )
}

fn clickup_task_comments_url(task_id: &str, custom_task_query: Option<&str>) -> String {
    append_query(
        format!(
            "{CLICKUP_API_BASE}/task/{}/comment",
            percent_encode_path_segment(task_id)
        ),
        custom_task_query,
    )
}

fn clickup_custom_task_id_query(team_id: &str) -> String {
    format!("custom_task_ids=true&team_id={}", percent_encode(team_id))
}

fn append_query(url: String, query: Option<&str>) -> String {
    match query {
        Some(query) => format!("{url}?{query}"),
        None => url,
    }
}

async fn fetch_clickup_comment_replies<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    comment_id: &str,
) -> Result<Vec<ClickUpComment>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/comment/{}/reply",
        percent_encode_path_segment(comment_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(clickup_comments_from_value(&value))
}

pub(crate) async fn fetch_space_statuses<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    space_id: &str,
) -> Result<Vec<ClickUpStatus>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/space/{}",
        percent_encode_path_segment(space_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(statuses_from_value(&value))
}

pub(crate) async fn fetch_folder_statuses<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    folder_id: &str,
) -> Result<Vec<ClickUpStatus>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/folder/{}",
        percent_encode_path_segment(folder_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(statuses_from_value(&value))
}

pub(crate) async fn fetch_list_statuses<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    list_id: &str,
) -> Result<Vec<ClickUpStatus>, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/list/{}",
        percent_encode_path_segment(list_id)
    );
    let value = client.request_json(Method::GET, url, token, None).await?;
    Ok(statuses_from_value(&value))
}

pub(crate) async fn put_task_status<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
    status_name: &str,
) -> Result<(), String> {
    let url = format!(
        "{CLICKUP_API_BASE}/task/{}",
        percent_encode_path_segment(task_id)
    );
    client
        .request_json(
            Method::PUT,
            url,
            token,
            Some(json!({ "status": status_name })),
        )
        .await
        .map(|_| ())
}

pub(crate) async fn assign_task_to_user<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
) -> Result<ClickUpUser, String> {
    let user = fetch_current_user(client, token).await?;
    let url = format!(
        "{CLICKUP_API_BASE}/task/{}",
        percent_encode_path_segment(task_id)
    );
    client
        .request_json(
            Method::PUT,
            url,
            token,
            Some(json!({ "assignees": { "add": [user.id] } })),
        )
        .await?;
    Ok(user)
}

pub(crate) async fn clear_task_assignees<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
) -> Result<(), String> {
    let task_url = format!(
        "{CLICKUP_API_BASE}/task/{}",
        percent_encode_path_segment(task_id)
    );
    let task = client
        .request_json(Method::GET, task_url.clone(), token, None)
        .await?;
    let assignee_ids: Vec<i64> = task
        .get("assignees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|assignee| assignee.get("id").and_then(Value::as_i64))
        .collect();
    if assignee_ids.is_empty() {
        return Ok(());
    }
    client
        .request_json(
            Method::PUT,
            task_url,
            token,
            Some(json!({ "assignees": { "rem": assignee_ids } })),
        )
        .await
        .map(|_| ())
}

pub(crate) async fn create_task_comment<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
    body_markdown: &str,
) -> Result<ClickUpComment, String> {
    let url = format!(
        "{CLICKUP_API_BASE}/task/{}/comment",
        percent_encode_path_segment(task_id)
    );
    let value = client
        .request_json(
            Method::POST,
            url,
            token,
            Some(json!({ "comment_text": body_markdown })),
        )
        .await?;
    Ok(ClickUpComment {
        id: value
            .get("id")
            .and_then(json_scalar_to_string)
            .unwrap_or_default(),
        body: body_markdown.to_string(),
        author_id: None,
        author_name: None,
        created_at: value.get("date").and_then(clickup_timestamp_to_rfc3339),
        attachments: Vec::new(),
        replies: Vec::new(),
    })
}

/// Reconciles a task's tags to exactly `desired`: removes current tags absent
/// from the desired set and adds desired tags not already present. ClickUp adds
/// and removes tags one at a time (`POST`/`DELETE /task/{id}/tag/{name}`).
pub(crate) async fn apply_task_tags<C: ClickUpJsonRequester + ?Sized>(
    client: &C,
    token: &str,
    task_id: &str,
    desired: Vec<String>,
) -> Result<(), String> {
    let task_url = format!(
        "{CLICKUP_API_BASE}/task/{}",
        percent_encode_path_segment(task_id)
    );
    let task = client
        .request_json(Method::GET, task_url, token, None)
        .await?;
    let current = collect_tag_names(&task);

    for name in &current {
        if !desired
            .iter()
            .any(|wanted| wanted.eq_ignore_ascii_case(name))
        {
            let url = format!(
                "{CLICKUP_API_BASE}/task/{}/tag/{}",
                percent_encode_path_segment(task_id),
                percent_encode_path_segment(name)
            );
            client
                .request_json(Method::DELETE, url, token, None)
                .await?;
        }
    }
    for name in &desired {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !current
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(trimmed))
        {
            let url = format!(
                "{CLICKUP_API_BASE}/task/{}/tag/{}",
                percent_encode_path_segment(task_id),
                percent_encode_path_segment(trimmed)
            );
            client.request_json(Method::POST, url, token, None).await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Value mapping helpers
// ---------------------------------------------------------------------------

fn filtered_tasks_url(
    team_id: &str,
    space_ids: &[String],
    assignee_ids: &[i64],
    page: usize,
) -> String {
    let mut url = format!(
        "{CLICKUP_API_BASE}/team/{}/task?page={page}&order_by=updated&reverse=true&include_closed=true&subtasks=true",
        percent_encode_path_segment(team_id)
    );
    for space_id in space_ids {
        // ClickUp expects repeated `space_ids[]=` params; encode the brackets so
        // the query parses as a valid URI.
        url.push_str(&format!("&space_ids%5B%5D={}", percent_encode(space_id)));
    }
    for assignee_id in assignee_ids {
        url.push_str(&format!("&assignees%5B%5D={assignee_id}"));
    }
    url
}

fn list_tasks_url(list_id: &str, assignee_ids: &[i64], page: usize) -> String {
    let mut url = format!(
        "{CLICKUP_API_BASE}/list/{}/task?page={page}&order_by=updated&reverse=true&include_closed=true&subtasks=true",
        percent_encode_path_segment(list_id)
    );
    for assignee_id in assignee_ids {
        url.push_str(&format!("&assignees%5B%5D={assignee_id}"));
    }
    url
}

fn user_from_value(value: &Value) -> Option<ClickUpUser> {
    let value = value
        .get("user")
        .or_else(|| value.get("member"))
        .unwrap_or(value);
    Some(ClickUpUser {
        id: clickup_user_id(value)?,
        username: opt_str(value, "username").or_else(|| opt_str(value, "name")),
        email: opt_str(value, "email"),
    })
}

fn workspace_from_value(value: &Value) -> Option<ClickUpWorkspace> {
    Some(ClickUpWorkspace {
        id: opt_str(value, "id")?,
        name: opt_str(value, "name").unwrap_or_default(),
        color: opt_str(value, "color"),
    })
}

fn space_from_value(value: &Value) -> Option<ClickUpSpace> {
    Some(ClickUpSpace {
        id: opt_str(value, "id")?,
        name: opt_str(value, "name").unwrap_or_default(),
        private: value
            .get("private")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn folder_from_value(value: &Value, fallback_space_id: Option<&str>) -> Option<ClickUpFolder> {
    Some(ClickUpFolder {
        id: opt_str(value, "id")?,
        name: opt_str(value, "name").unwrap_or_default(),
        space_id: value
            .get("space")
            .and_then(|space| opt_str(space, "id"))
            .or_else(|| opt_str(value, "space_id"))
            .or_else(|| fallback_space_id.map(str::to_string)),
    })
}

fn list_from_value(
    value: &Value,
    fallback_folder_id: Option<&str>,
    fallback_space_id: Option<&str>,
) -> Option<ClickUpList> {
    Some(ClickUpList {
        id: opt_str(value, "id")?,
        name: opt_str(value, "name").unwrap_or_default(),
        folder_id: value
            .get("folder")
            .and_then(|folder| opt_str(folder, "id"))
            .or_else(|| opt_str(value, "folder_id"))
            .or_else(|| fallback_folder_id.map(str::to_string)),
        space_id: value
            .get("space")
            .and_then(|space| opt_str(space, "id"))
            .or_else(|| opt_str(value, "space_id"))
            .or_else(|| fallback_space_id.map(str::to_string)),
    })
}

fn status_from_value(value: &Value) -> Option<ClickUpStatus> {
    let status = opt_str(value, "status")?;
    let status_type = opt_str(value, "type").unwrap_or_default();
    let category = map_status_type_to_category(&status_type);
    Some(ClickUpStatus {
        id: opt_str(value, "id"),
        status,
        status_type,
        category,
        color: opt_str(value, "color"),
        orderindex: value.get("orderindex").and_then(Value::as_i64),
    })
}

fn statuses_from_value(value: &Value) -> Vec<ClickUpStatus> {
    value
        .get("statuses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(status_from_value)
        .collect()
}

fn task_summary_from_value(task: &Value) -> Option<ClickUpTaskSummary> {
    let id = opt_str(task, "id")?;
    let status = task.get("status");
    let status_type = status.and_then(|value| opt_str(value, "type"));
    Some(ClickUpTaskSummary {
        id,
        custom_id: opt_str(task, "custom_id"),
        name: opt_str(task, "name").unwrap_or_default(),
        url: opt_str(task, "url"),
        status_name: status.and_then(|value| opt_str(value, "status")),
        status_category: status_type.as_deref().map(map_status_type_to_category),
        status_type,
        status_color: status.and_then(|value| opt_str(value, "color")),
        assignees: collect_assignee_names(task),
        assignee_ids: collect_assignee_ids(task),
        watchers: collect_clickup_users_from_fields(task, &["watchers", "followers"]),
        tags: collect_tag_names(task),
        sprint_names: collect_location_names(task),
        location_ids: collect_location_ids(task),
        location_folder_ids: collect_location_folder_ids(task),
        location_space_ids: collect_location_space_ids(task),
        space_id: task.get("space").and_then(|value| opt_str(value, "id")),
        folder_id: task.get("folder").and_then(|value| opt_str(value, "id")),
        list_id: task.get("list").and_then(|value| opt_str(value, "id")),
        list_name: task.get("list").and_then(|value| opt_str(value, "name")),
        updated_at: task
            .get("date_updated")
            .and_then(clickup_timestamp_to_rfc3339),
    })
}

fn task_summary_matches_query(summary: &ClickUpTaskSummary, query: &str) -> bool {
    let scalar_fields = [
        Some(summary.id.as_str()),
        summary.custom_id.as_deref(),
        Some(summary.name.as_str()),
        summary.status_name.as_deref(),
        summary.list_name.as_deref(),
    ];
    scalar_fields
        .into_iter()
        .flatten()
        .any(|value| value.to_ascii_lowercase().contains(query))
        || summary
            .tags
            .iter()
            .chain(summary.assignees.iter())
            .any(|value| value.to_ascii_lowercase().contains(query))
        || summary
            .watchers
            .iter()
            .any(|user| clickup_user_matches_query(user, query))
}

fn task_content_from_value(task: &Value) -> Option<ClickUpTaskContent> {
    let id = opt_str(task, "id")?;
    let status = task.get("status");
    let status_type = status.and_then(|value| opt_str(value, "type"));
    let description = opt_str(task, "description")
        .or_else(|| opt_str(task, "text_content"))
        .unwrap_or_default();
    Some(ClickUpTaskContent {
        id,
        custom_id: opt_str(task, "custom_id"),
        name: opt_str(task, "name").unwrap_or_default(),
        url: opt_str(task, "url"),
        description,
        status_name: status.and_then(|value| opt_str(value, "status")),
        status_category: status_type.as_deref().map(map_status_type_to_category),
        status_type,
        creator: task
            .get("creator")
            .and_then(|value| opt_str(value, "username")),
        assignees: collect_assignee_names(task),
        watchers: collect_clickup_users_from_fields(task, &["watchers", "followers"]),
        tags: collect_tag_names(task),
        comments: Vec::new(),
        attachments: collect_clickup_attachments(task),
        updated_at: task
            .get("date_updated")
            .and_then(clickup_timestamp_to_rfc3339),
        space_id: task.get("space").and_then(|value| opt_str(value, "id")),
        list_name: task.get("list").and_then(|value| opt_str(value, "name")),
    })
}

fn clickup_comments_from_value(value: &Value) -> Vec<ClickUpComment> {
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    comments
        .into_iter()
        .flatten()
        .filter_map(clickup_comment_from_value)
        .collect()
}

fn clickup_comment_from_value(value: &Value) -> Option<ClickUpComment> {
    let id = value.get("id").and_then(json_scalar_to_string)?;
    let body = clickup_comment_body(value)?;
    Some(ClickUpComment {
        id,
        body,
        author_id: value
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(Value::as_i64),
        author_name: value.get("user").and_then(clickup_user_display_name),
        created_at: value.get("date").and_then(clickup_timestamp_to_rfc3339),
        attachments: collect_clickup_attachments(value),
        replies: Vec::new(),
    })
}

fn clickup_comment_reply_count(comments_payload: &Value, comment_id: &str) -> i64 {
    comments_payload
        .get("comments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|comment| {
            comment.get("id").and_then(json_scalar_to_string).as_deref() == Some(comment_id)
        })
        .and_then(|comment| comment.get("reply_count"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

fn clickup_comment_body(value: &Value) -> Option<String> {
    opt_str(value, "comment_text")
        .or_else(|| opt_str(value, "text_content"))
        .or_else(|| opt_str(value, "body"))
        .or_else(|| opt_str(value, "comment"))
        .or_else(|| {
            value
                .get("comment")
                .and_then(Value::as_array)
                .map(|parts| {
                    parts
                        .iter()
                        .filter_map(|part| opt_str(part, "text"))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .filter(|text| !text.trim().is_empty())
        })
}

fn clickup_user_display_name(value: &Value) -> Option<String> {
    opt_str(value, "username")
        .or_else(|| opt_str(value, "email"))
        .or_else(|| opt_str(value, "name"))
}

fn clickup_user_id(value: &Value) -> Option<i64> {
    value
        .get("id")
        .and_then(Value::as_i64)
        .or_else(|| opt_str(value, "id").and_then(|id| id.parse::<i64>().ok()))
}

fn clickup_user_matches_query(user: &ClickUpUser, query: &str) -> bool {
    user.id.to_string().contains(query)
        || user
            .username
            .as_deref()
            .is_some_and(|name| name.to_ascii_lowercase().contains(query))
        || user
            .email
            .as_deref()
            .is_some_and(|email| email.to_ascii_lowercase().contains(query))
}

fn collect_clickup_users_from_fields(task: &Value, fields: &[&str]) -> Vec<ClickUpUser> {
    let mut seen = HashSet::new();
    let mut users = Vec::new();
    for field in fields {
        for user in task
            .get(*field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(user_from_value)
        {
            if seen.insert(user.id) {
                users.push(user);
            }
        }
    }
    users
}

fn collect_assignee_names(task: &Value) -> Vec<String> {
    task.get("assignees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|assignee| opt_str(assignee, "username").or_else(|| opt_str(assignee, "email")))
        .collect()
}

fn collect_assignee_ids(task: &Value) -> Vec<i64> {
    task.get("assignees")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|assignee| assignee.get("id").and_then(Value::as_i64))
        .collect()
}

fn collect_clickup_attachments(task: &Value) -> Vec<ClickUpAttachment> {
    task.get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(clickup_attachment_from_value)
        .collect()
}

fn clickup_attachment_from_value(value: &Value) -> Option<ClickUpAttachment> {
    let filename = opt_str(value, "filename")
        .or_else(|| opt_str(value, "title"))
        .or_else(|| opt_str(value, "name"))?;
    Some(ClickUpAttachment {
        id: value
            .get("id")
            .and_then(json_scalar_to_string)
            .or_else(|| opt_str(value, "uuid")),
        filename,
        mime_type: opt_str(value, "mime_type")
            .or_else(|| opt_str(value, "mimeType"))
            .or_else(|| opt_str(value, "content_type")),
        size: value
            .get("size")
            .and_then(Value::as_i64)
            .or_else(|| value.get("file_size").and_then(Value::as_i64)),
        url: opt_str(value, "url")
            .or_else(|| opt_str(value, "download_url"))
            .or_else(|| opt_str(value, "downloadUrl")),
    })
}

fn collect_tag_names(task: &Value) -> Vec<String> {
    task.get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tag| opt_str(tag, "name"))
        .collect()
}

fn collect_location_names(task: &Value) -> Vec<String> {
    task.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| opt_str(location, "name"))
        .collect()
}

fn collect_location_ids(task: &Value) -> Vec<String> {
    task.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| opt_str(location, "id"))
        .collect()
}

fn collect_location_folder_ids(task: &Value) -> Vec<String> {
    task.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            location
                .get("folder")
                .and_then(|folder| opt_str(folder, "id"))
                .or_else(|| opt_str(location, "folder_id"))
        })
        .collect()
}

fn collect_location_space_ids(task: &Value) -> Vec<String> {
    task.get("locations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|location| {
            location
                .get("space")
                .and_then(|space| opt_str(space, "id"))
                .or_else(|| opt_str(location, "space_id"))
        })
        .collect()
}

fn opt_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

/// Stringifies a scalar JSON value (ClickUp returns ids/dates as either strings
/// or numbers depending on the endpoint).
fn json_scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn clickup_timestamp_to_rfc3339(value: &Value) -> Option<String> {
    let raw = json_scalar_to_string(value)?;
    if !raw.chars().all(|char| char.is_ascii_digit()) {
        return Some(raw);
    }
    let millis = raw.parse::<i64>().ok()?;
    DateTime::<Utc>::from_timestamp_millis(millis).map(|date| date.to_rfc3339())
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn percent_encode_path_segment(value: &str) -> String {
    percent_encode(value)
}
