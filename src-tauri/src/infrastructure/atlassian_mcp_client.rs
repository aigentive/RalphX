//! Atlassian operations that only the MCP tool surface needs: Jira issue
//! create/update, Confluence page read/create/update, and a bounded generic API
//! proxy.
//!
//! Every call goes through [`AtlassianJsonRequester`] and
//! [`HyperAtlassianApiClient::resource_url`], so API-token and OAuth modes plus
//! server-side token refresh are reused unchanged.

use hyper::Method;
use serde_json::Value;

use crate::domain::integrations::{
    validate_atlassian_raw_path, AtlassianApiError, AtlassianRawMethod, ConfluencePageContent,
    ConfluencePageCreateRequest, ConfluencePageUpdateRequest, JiraIssueCreateRequest,
    JiraIssueCreated, JiraIssueUpdateRequest, ATLASSIAN_RAW_RESPONSE_MAX_BYTES,
};

use super::adf_markdown_writer::{markdown_to_adf, markdown_to_storage};
use super::atlassian_client::{
    request_auth, AtlassianAuthContext, AtlassianJsonRequester, AtlassianResourceKind,
    HyperAtlassianApiClient,
};

fn required(value: &str, message: &str) -> Result<String, AtlassianApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AtlassianApiError::transport(message));
    }
    Ok(trimmed.to_string())
}

fn encode_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Create a Jira issue via the Cloud v3 REST API.
pub(crate) async fn create_jira_issue<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    request: &JiraIssueCreateRequest,
) -> Result<JiraIssueCreated, AtlassianApiError> {
    let project_key = required(&request.project_key, "Jira project key is required")?;
    let issue_type = required(&request.issue_type, "Jira issue type is required")?;
    let summary = required(&request.summary, "Jira issue summary is required")?;

    let mut fields = serde_json::Map::new();
    fields.insert(
        "project".to_string(),
        serde_json::json!({ "key": project_key }),
    );
    fields.insert(
        "issuetype".to_string(),
        serde_json::json!({ "name": issue_type }),
    );
    fields.insert("summary".to_string(), Value::String(summary));
    if let Some(description) = request.description.as_deref() {
        fields.insert("description".to_string(), markdown_to_adf(description));
    }
    if !request.labels.is_empty() {
        fields.insert("labels".to_string(), serde_json::json!(request.labels));
    }
    if let Some(priority) = request.priority.as_deref() {
        fields.insert(
            "priority".to_string(),
            serde_json::json!({ "name": priority }),
        );
    }
    if let Some(parent_key) = request.parent_key.as_deref() {
        fields.insert(
            "parent".to_string(),
            serde_json::json!({ "key": parent_key }),
        );
    }
    if let Some(assignee_account_id) = request.assignee_account_id.as_deref() {
        fields.insert(
            "assignee".to_string(),
            serde_json::json!({ "accountId": assignee_account_id }),
        );
    }
    if !request.components.is_empty() {
        let components = request
            .components
            .iter()
            .map(|name| serde_json::json!({ "name": name }))
            .collect::<Vec<_>>();
        fields.insert("components".to_string(), Value::Array(components));
    }

    let value = client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                "/rest/api/3/issue",
            ),
            request_auth(auth),
            Some(Value::Object(
                [("fields".to_string(), Value::Object(fields))]
                    .into_iter()
                    .collect(),
            )),
        )
        .await?;

    let key = value
        .get("key")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AtlassianApiError::transport("Jira create response did not include an issue key")
        })?
        .to_string();
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(&key)
        .to_string();

    Ok(JiraIssueCreated {
        url: format!("{}/browse/{}", auth.site_url.trim_end_matches('/'), key),
        id,
        key,
    })
}

/// Update a bounded subset of Jira issue fields.
pub(crate) async fn update_jira_issue<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    request: &JiraIssueUpdateRequest,
) -> Result<(), AtlassianApiError> {
    let issue_key = required(issue_key, "Jira issue key is required")?;
    if request.is_empty() {
        return Err(AtlassianApiError::transport(
            "Jira issue update requires at least one field",
        ));
    }

    let mut fields = serde_json::Map::new();
    if let Some(summary) = request.summary.as_deref() {
        fields.insert("summary".to_string(), Value::String(summary.to_string()));
    }
    if let Some(description) = request.description.as_deref() {
        fields.insert("description".to_string(), markdown_to_adf(description));
    }
    if let Some(labels) = request.labels.as_ref() {
        fields.insert("labels".to_string(), serde_json::json!(labels));
    }
    if let Some(priority) = request.priority.as_deref() {
        fields.insert(
            "priority".to_string(),
            serde_json::json!({ "name": priority }),
        );
    }

    client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!("/rest/api/3/issue/{}", encode_segment(&issue_key)),
            ),
            request_auth(auth),
            Some(Value::Object(
                [("fields".to_string(), Value::Object(fields))]
                    .into_iter()
                    .collect(),
            )),
        )
        .await?;
    Ok(())
}

fn confluence_page_from_value(
    value: &Value,
    site_url: &str,
) -> Result<ConfluencePageContent, AtlassianApiError> {
    let id = value
        .get("id")
        .and_then(|id| {
            id.as_str()
                .map(str::to_string)
                .or_else(|| id.as_i64().map(|id| id.to_string()))
        })
        .ok_or_else(|| {
            AtlassianApiError::transport("Confluence response did not include a page id")
        })?;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let version = value
        .get("version")
        .and_then(|version| version.get("number"))
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let body_storage = value
        .get("body")
        .and_then(|body| body.get("storage"))
        .and_then(|storage| storage.get("value"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let space_id = value.get("spaceId").and_then(|space| {
        space
            .as_str()
            .map(str::to_string)
            .or_else(|| space.as_i64().map(|space| space.to_string()))
    });
    let url = value
        .get("_links")
        .and_then(|links| links.get("webui"))
        .and_then(Value::as_str)
        .map(|path| format!("{}/wiki{}", site_url.trim_end_matches('/'), path));

    Ok(ConfluencePageContent {
        id,
        title,
        space_id,
        version,
        body_storage,
        url,
    })
}

/// Read a Confluence page including its storage-format body.
pub(crate) async fn confluence_get_page<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
) -> Result<ConfluencePageContent, AtlassianApiError> {
    let page_id = required(page_id, "Confluence page id is required")?;
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}?body-format=storage",
                    encode_segment(&page_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    confluence_page_from_value(&value, &auth.site_url)
}

/// Resolve a Confluence page body from the caller-supplied storage/markdown
/// pair. `Ok(None)` means neither was supplied (only valid for updates, where
/// it means "leave the current body untouched").
///
/// Callers at the HTTP boundary already reject a "both supplied" request with
/// a typed `InvalidRequest`; this is a defense-in-depth check for any other
/// caller of these client functions.
fn resolve_confluence_body_storage(
    body_storage: Option<&str>,
    body_markdown: Option<&str>,
) -> Result<Option<String>, AtlassianApiError> {
    match (body_storage, body_markdown) {
        (Some(_), Some(_)) => Err(AtlassianApiError::transport(
            "Confluence page body must be either bodyStorage or bodyMarkdown, not both",
        )),
        (Some(storage), None) => Ok(Some(storage.to_string())),
        (None, Some(markdown)) => Ok(Some(markdown_to_storage(markdown))),
        (None, None) => Ok(None),
    }
}

/// Create a Confluence page from storage-format or markdown content.
pub(crate) async fn confluence_create_page<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    request: &ConfluencePageCreateRequest,
) -> Result<ConfluencePageContent, AtlassianApiError> {
    let space_id = required(&request.space_id, "Confluence space id is required")?;
    let title = required(&request.title, "Confluence page title is required")?;
    let body_storage = resolve_confluence_body_storage(
        request.body_storage.as_deref(),
        request.body_markdown.as_deref(),
    )?
    .ok_or_else(|| {
        AtlassianApiError::transport(
            "Confluence page body is required: supply bodyStorage or bodyMarkdown",
        )
    })?;

    let mut body = serde_json::Map::new();
    body.insert("spaceId".to_string(), Value::String(space_id));
    body.insert("status".to_string(), Value::String("current".to_string()));
    body.insert("title".to_string(), Value::String(title));
    body.insert(
        "body".to_string(),
        serde_json::json!({
            "representation": "storage",
            "value": body_storage,
        }),
    );
    if let Some(parent_id) = request.parent_id.as_deref() {
        body.insert("parentId".to_string(), Value::String(parent_id.to_string()));
    }

    let value = client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                "/wiki/api/v2/pages",
            ),
            request_auth(auth),
            Some(Value::Object(body)),
        )
        .await?;
    confluence_page_from_value(&value, &auth.site_url)
}

/// Update a Confluence page, reading the current version first and
/// incrementing it as Confluence's optimistic-concurrency contract requires.
pub(crate) async fn confluence_update_page<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
    request: &ConfluencePageUpdateRequest,
) -> Result<ConfluencePageContent, AtlassianApiError> {
    let page_id = required(page_id, "Confluence page id is required")?;
    if request.is_empty() {
        return Err(AtlassianApiError::transport(
            "Confluence page update requires a title or body",
        ));
    }
    // Validated before any network call: a "both supplied" conflict must not
    // spend a GET first.
    let resolved_body_storage = resolve_confluence_body_storage(
        request.body_storage.as_deref(),
        request.body_markdown.as_deref(),
    )?;

    let current = confluence_get_page(client, auth, &page_id).await?;
    let title = request
        .title
        .clone()
        .unwrap_or_else(|| current.title.clone());
    let body_storage = resolved_body_storage.unwrap_or_else(|| current.body_storage.clone());

    let value = client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!("/wiki/api/v2/pages/{}", encode_segment(&page_id)),
            ),
            request_auth(auth),
            Some(serde_json::json!({
                "id": page_id,
                "status": "current",
                "title": title,
                "body": {
                    "representation": "storage",
                    "value": body_storage,
                },
                "version": { "number": current.version + 1 },
            })),
        )
        .await?;
    confluence_page_from_value(&value, &auth.site_url)
}

/// Issue a bounded generic Atlassian API request.
///
/// The caller's tier is enforced upstream; this layer independently re-validates
/// the path at the request sink and bounds the response size.
pub(crate) async fn raw_api_request<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    method: AtlassianRawMethod,
    kind: AtlassianResourceKind,
    path: &str,
    body: Option<Value>,
) -> Result<Value, AtlassianApiError> {
    // Re-validate at the request sink: the caller may have validated already,
    // but containment proof must be local to the sink.
    let path = validate_atlassian_raw_path(path).map_err(AtlassianApiError::transport)?;
    let http_method = Method::from_bytes(method.as_str().as_bytes())
        .map_err(|_| AtlassianApiError::transport("Unsupported Atlassian HTTP method"))?;
    let body = if method.is_read_only() { None } else { body };

    let value = client
        .request_json(
            http_method,
            HyperAtlassianApiClient::resource_url(auth, kind, path),
            request_auth(auth),
            body,
        )
        .await?;

    let encoded_len = serde_json::to_vec(&value)
        .map(|bytes| bytes.len())
        .unwrap_or(0);
    if encoded_len > ATLASSIAN_RAW_RESPONSE_MAX_BYTES {
        return Err(AtlassianApiError::transport(format!(
            "Atlassian response exceeded the {ATLASSIAN_RAW_RESPONSE_MAX_BYTES} byte limit"
        )));
    }
    Ok(value)
}
