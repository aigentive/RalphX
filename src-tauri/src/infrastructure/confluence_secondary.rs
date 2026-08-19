//! Confluence reference-depth secondary fetchers (Phase 5): footer comments,
//! attachments, and child pages. Each is an independent, capped, best-effort
//! call issued after the primary page fetch in `atlassian_client::fetch_confluence`
//! — a failure here must never fail the page fetch itself, so every fetcher
//! returns a status-preserving [`AtlassianApiError`] for the caller to log and
//! swallow per-section. Kept as a sibling module (not in `atlassian_client.rs`,
//! already 4x the file-size limit) generic over `AtlassianJsonRequester`, with
//! a `?Sized` bound so `fetch_confluence`'s own erased client can be forwarded.

use hyper::Method;
use serde_json::Value;

use crate::domain::integrations::{
    AtlassianApiError, AtlassianJiraAttachment, AtlassianJiraChildIssue, AtlassianJiraComment,
    AtlassianResourceContent,
};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_client::{
    percent_encode_path_segment, request_auth, strip_html, AtlassianAuthContext,
    AtlassianJsonRequester, AtlassianResourceKind, HyperAtlassianApiClient,
};

/// Rendered in the Confluence page body (`fetch_confluence`); kept in sync
/// with the last-N shown there.
pub(crate) const CONFLUENCE_FOOTER_COMMENT_FETCH_LIMIT: usize = 10;
pub(crate) const CONFLUENCE_ATTACHMENT_FETCH_LIMIT: usize = 25;
pub(crate) const CONFLUENCE_CHILD_PAGE_FETCH_LIMIT: usize = 25;

const CONFLUENCE_LINK_NOT_EXPANDABLE_NOTE: &str =
    "This Confluence content type is not expandable inline; open the link to view it.";

pub(crate) async fn fetch_confluence_footer_comments<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
) -> Result<Vec<AtlassianJiraComment>, AtlassianApiError> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}/footer-comments?limit={CONFLUENCE_FOOTER_COMMENT_FETCH_LIMIT}&body-format=storage",
                    percent_encode_path_segment(page_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(confluence_comment_from_value)
        .take(CONFLUENCE_FOOTER_COMMENT_FETCH_LIMIT)
        .collect())
}

fn confluence_comment_from_value(value: &Value) -> Option<AtlassianJiraComment> {
    let body_html = value
        .get("body")
        .and_then(|body| body.get("storage"))
        .and_then(|storage| storage.get("value"))
        .and_then(Value::as_str)?;
    let body_text = strip_html(body_html);
    if body_text.trim().is_empty() {
        return None;
    }
    let version = value.get("version");
    let created_at = version
        .and_then(|version| version.get("createdAt"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(AtlassianJiraComment {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        author: version
            .and_then(|version| version.get("authorId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        body_markdown: body_text.clone(),
        body_text,
        created_at: created_at.clone(),
        updated_at: created_at,
    })
}

pub(crate) async fn fetch_confluence_attachments<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
) -> Result<Vec<AtlassianJiraAttachment>, AtlassianApiError> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}/attachments?limit={CONFLUENCE_ATTACHMENT_FETCH_LIMIT}",
                    percent_encode_path_segment(page_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(confluence_attachment_from_value)
        .take(CONFLUENCE_ATTACHMENT_FETCH_LIMIT)
        .collect())
}

fn confluence_attachment_from_value(value: &Value) -> Option<AtlassianJiraAttachment> {
    let filename = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    Some(AtlassianJiraAttachment {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        filename,
        mime_type: value
            .get("mediaType")
            .and_then(Value::as_str)
            .map(str::to_string),
        size: value.get("fileSize").and_then(Value::as_i64),
        author: value
            .get("version")
            .and_then(|version| version.get("authorId"))
            .and_then(Value::as_str)
            .map(str::to_string),
        content_url: value
            .get("downloadLink")
            .and_then(Value::as_str)
            .map(str::to_string),
        thumbnail_url: None,
        created_at: value
            .get("createdAt")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(crate) async fn fetch_confluence_child_pages<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
) -> Result<Vec<AtlassianJiraChildIssue>, AtlassianApiError> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}/children?limit={CONFLUENCE_CHILD_PAGE_FETCH_LIMIT}",
                    percent_encode_path_segment(page_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(confluence_child_page_from_value)
        .take(CONFLUENCE_CHILD_PAGE_FETCH_LIMIT)
        .collect())
}

fn confluence_child_page_from_value(value: &Value) -> Option<AtlassianJiraChildIssue> {
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(untitled)")
        .to_string();
    Some(AtlassianJiraChildIssue {
        key: id,
        summary: title,
        status: None,
        issue_type: None,
    })
}

/// Builds a titled, not-expandable reference for Confluence content the API
/// cannot fetch a body for (whiteboards, databases): no network call, just
/// title + URL + an explicit note.
pub(crate) fn confluence_link_content(
    reference: &ComposerIntegrationReference,
) -> Result<AtlassianResourceContent, String> {
    let title = reference
        .title
        .clone()
        .unwrap_or_else(|| format!("Confluence link {}", reference.id));
    let url = reference.url.clone();
    let mut body = vec![title.clone()];
    if let Some(url) = url.as_deref() {
        body.push(url.to_string());
    }
    body.push(CONFLUENCE_LINK_NOT_EXPANDABLE_NOTE.to_string());
    Ok(AtlassianResourceContent {
        kind: AtlassianResourceKind::Confluence,
        id: reference.id.clone(),
        key: None,
        title,
        url,
        body: body.join("\n"),
        status: None,
        assignee: None,
        reporter: None,
        updated_at_remote: None,
        description_markdown: None,
        description_text: None,
        acceptance_criteria_markdown: None,
        acceptance_criteria_text: None,
        comments: Vec::new(),
        attachments: Vec::new(),
        issue_type: None,
        labels: Vec::new(),
        priority: None,
        parent_key: None,
        children: Vec::new(),
    })
}
