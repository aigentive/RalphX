use async_trait::async_trait;
use base64::Engine;
use http_body_util::{BodyExt, Full};
use hyper::{Method, Request};
use hyper_rustls::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use serde_json::Value;
use tokio::time::Duration;
use tokio_util::bytes::Bytes;

use crate::domain::integrations::{
    AtlassianApiClient, AtlassianApiError, AtlassianAuthContext as ApplicationAtlassianAuthContext,
    AtlassianConnectivity, AtlassianCredential as ApplicationAtlassianCredential,
    AtlassianJiraAttachment, AtlassianJiraChildIssue, AtlassianJiraComment,
    AtlassianJiraTransition, AtlassianOAuthResource, AtlassianOAuthTokenResponse,
    AtlassianRawMethod, AtlassianResourceContent,
    AtlassianResourceKind as ApplicationAtlassianResourceKind, AtlassianResourceSummary,
    ConfluencePageContent, ConfluencePageCreateRequest, ConfluencePageUpdateRequest,
    ConfluenceSpaceSummary, JiraBoardColumn, JiraBoardConfiguration, JiraBoardSummary,
    JiraCommentsPage, JiraIssueCreateRequest, JiraIssueCreated, JiraIssueDetail,
    JiraIssueUpdateRequest, JiraProjectSummary, JiraSprintSummary, JiraStatusSummary,
    JiraUserSummary,
};
use crate::domain::services::ComposerIntegrationReference;

use super::atlassian_jira_fields::{acceptance_criteria_from_fields, JiraFieldCatalogCache};
use super::atlassian_mcp_client;
use super::confluence_secondary;
use super::jira_agile_client::{
    get_jira_board_configuration, list_jira_active_sprints, list_jira_boards,
};
use super::jira_board_context_client;

pub(crate) type JiraAgileAuthContext = AtlassianAuthContext;
#[cfg(test)]
pub(crate) type JiraAgileCredential = AtlassianCredential;
pub(crate) type JiraAgileResourceKind = AtlassianResourceKind;
pub(crate) type JiraAgileBoardColumn = JiraBoardColumn;
pub(crate) type JiraAgileBoardConfiguration = JiraBoardConfiguration;
pub(crate) type JiraAgileBoardSummary = JiraBoardSummary;
pub(crate) type JiraAgileSprintSummary = JiraSprintSummary;
pub(crate) type AtlassianAuthContext = ApplicationAtlassianAuthContext;
pub(crate) type AtlassianCredential = ApplicationAtlassianCredential;
pub(crate) type AtlassianResourceKind = ApplicationAtlassianResourceKind;

const JIRA_PROJECT_SEARCH_PAGE_SIZE: usize = 100;
const JIRA_ISSUE_SEARCH_PAGE_SIZE: usize = 100;

pub struct HyperAtlassianApiClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    timeout: Duration,
    jira_field_catalog: JiraFieldCatalogCache,
}

impl HyperAtlassianApiClient {
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
            jira_field_catalog: JiraFieldCatalogCache::new(),
        })
    }

    pub(crate) fn resource_url(
        auth: &AtlassianAuthContext,
        kind: AtlassianResourceKind,
        path: &str,
    ) -> String {
        match &auth.credential {
            AtlassianCredential::ApiToken { .. } => format!("{}{}", auth.site_url, path),
            AtlassianCredential::OAuth { cloud_id, .. } => {
                let product = match kind {
                    AtlassianResourceKind::Jira => "jira",
                    AtlassianResourceKind::Confluence => "confluence",
                };
                format!("https://api.atlassian.com/ex/{product}/{cloud_id}{path}")
            }
        }
    }

    async fn send_json_request(
        &self,
        method: Method,
        url: String,
        auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        let uri = url.parse::<hyper::Uri>().map_err(|error| {
            AtlassianApiError::transport(format!("Invalid Atlassian URL: {error}"))
        })?;
        let body_bytes = body
            .map(|value| serde_json::to_vec(&value))
            .transpose()
            .map_err(|error| AtlassianApiError::transport(error.to_string()))?
            .unwrap_or_default();
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Accept", "application/json");
        builder = match auth {
            RequestAuth::Basic { email, token } => {
                let auth =
                    base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"));
                builder.header("Authorization", format!("Basic {auth}"))
            }
            RequestAuth::Bearer(token) => {
                builder.header("Authorization", format!("Bearer {token}"))
            }
            RequestAuth::None => builder,
        };
        if !body_bytes.is_empty() {
            builder = builder.header("Content-Type", "application/json");
        }
        let request = builder
            .body(Full::new(Bytes::from(body_bytes)))
            .map_err(|error| {
                AtlassianApiError::transport(format!("Failed to build Atlassian request: {error}"))
            })?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| AtlassianApiError::transport("Atlassian request timed out"))?
            .map_err(|error| {
                AtlassianApiError::transport(format!("Atlassian request failed: {error}"))
            })?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| {
                AtlassianApiError::transport(format!("Failed to read Atlassian response: {error}"))
            })?
            .to_bytes();
        if !status.is_success() {
            // Auth lives in request headers, so the response body carries no
            // RalphX-held credential material.
            let excerpt = String::from_utf8_lossy(&bytes);
            return Err(AtlassianApiError::from_status(status.as_u16(), &excerpt));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|error| {
            AtlassianApiError::transport(format!("Failed to parse Atlassian response: {error}"))
        })
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

pub(crate) enum RequestAuth<'a> {
    Basic { email: &'a str, token: &'a str },
    Bearer(&'a str),
    None,
}

#[async_trait]
pub(crate) trait AtlassianJsonRequester: Send + Sync {
    /// Issue an Atlassian JSON request, preserving the HTTP status when
    /// Atlassian answered with a failure. Callers classify on
    /// [`AtlassianApiError::status`], never on message text.
    async fn request_json(
        &self,
        method: Method,
        url: String,
        auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError>;
}

#[async_trait]
impl AtlassianJsonRequester for HyperAtlassianApiClient {
    async fn request_json(
        &self,
        method: Method,
        url: String,
        auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        self.send_json_request(method, url, auth, body).await
    }
}

pub(crate) fn request_auth(auth: &AtlassianAuthContext) -> RequestAuth<'_> {
    match &auth.credential {
        AtlassianCredential::ApiToken { email, token } => RequestAuth::Basic { email, token },
        AtlassianCredential::OAuth { access_token, .. } => RequestAuth::Bearer(access_token),
    }
}

#[async_trait]
impl AtlassianApiClient for HyperAtlassianApiClient {
    async fn validate(&self, auth: &AtlassianAuthContext) -> Result<AtlassianConnectivity, String> {
        let jira_available = self
            .request_json(
                Method::GET,
                Self::resource_url(auth, AtlassianResourceKind::Jira, "/rest/api/3/myself"),
                request_auth(auth),
                None,
            )
            .await
            .is_ok();
        let confluence_available = self
            .request_json(
                Method::GET,
                Self::resource_url(
                    auth,
                    AtlassianResourceKind::Confluence,
                    "/wiki/rest/api/user/current",
                ),
                request_auth(auth),
                None,
            )
            .await
            .is_ok();
        if !jira_available && !confluence_available {
            return Err(
                "Atlassian credentials did not validate for Jira or Confluence".to_string(),
            );
        }
        Ok(AtlassianConnectivity {
            jira_available,
            confluence_available,
        })
    }

    async fn search(
        &self,
        auth: &AtlassianAuthContext,
        kind: AtlassianResourceKind,
        query: &str,
        limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, String> {
        match kind {
            AtlassianResourceKind::Jira => search_jira(self, auth, query, limit).await,
            AtlassianResourceKind::Confluence => search_confluence(self, auth, query, limit).await,
        }
    }

    async fn search_raw(
        &self,
        auth: &AtlassianAuthContext,
        kind: AtlassianResourceKind,
        query: &str,
        limit: usize,
    ) -> Result<Vec<AtlassianResourceSummary>, AtlassianApiError> {
        match kind {
            AtlassianResourceKind::Jira => search_jira_raw(self, auth, query, limit).await,
            AtlassianResourceKind::Confluence => {
                search_confluence_raw(self, auth, query, limit).await
            }
        }
    }

    async fn fetch(
        &self,
        auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        match reference.kind.as_str() {
            "jira" => {
                let field_ids = self
                    .jira_field_catalog
                    .acceptance_criteria_ids(self, auth)
                    .await;
                fetch_jira(self, auth, reference, &field_ids).await
            }
            "confluence" => fetch_confluence(self, auth, reference).await,
            "confluence_link" => confluence_secondary::confluence_link_content(reference),
            "jira_board" => {
                jira_board_context_client::fetch_jira_board_context(self, auth, reference).await
            }
            other => Err(format!("Unsupported Atlassian reference kind: {other}")),
        }
    }

    async fn assign_jira_issue_to_current_user(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        assign_jira_issue_to_current_user(self, auth, issue_key).await
    }

    async fn clear_jira_issue_assignee(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<(), String> {
        clear_jira_issue_assignee(self, auth, issue_key).await
    }

    async fn assign_jira_issue_to_account(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        account_id: &str,
    ) -> Result<(), String> {
        assign_jira_issue_to_account(self, auth, issue_key, account_id).await
    }

    async fn list_jira_issue_transitions(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
    ) -> Result<Vec<AtlassianJiraTransition>, String> {
        list_jira_issue_transitions(self, auth, issue_key).await
    }

    async fn transition_jira_issue(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        transition_id: &str,
    ) -> Result<(), String> {
        transition_jira_issue(self, auth, issue_key, transition_id).await
    }

    async fn add_jira_comment(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        body_markdown: &str,
    ) -> Result<AtlassianJiraComment, String> {
        add_jira_comment(self, auth, issue_key, body_markdown).await
    }

    async fn set_jira_issue_labels(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        labels: Vec<String>,
    ) -> Result<(), String> {
        set_jira_issue_labels(self, auth, issue_key, labels).await
    }

    async fn list_jira_projects(
        &self,
        auth: &AtlassianAuthContext,
        limit: usize,
    ) -> Result<Vec<JiraProjectSummary>, String> {
        list_jira_projects(self, auth, limit).await
    }

    async fn list_jira_project_statuses(
        &self,
        auth: &AtlassianAuthContext,
        project_key: &str,
    ) -> Result<Vec<JiraStatusSummary>, String> {
        list_jira_project_statuses(self, auth, project_key).await
    }

    async fn list_jira_project_issues(
        &self,
        auth: &AtlassianAuthContext,
        project_key: &str,
        limit: usize,
    ) -> Result<Vec<JiraIssueDetail>, String> {
        search_jira_by_project(self, auth, project_key, limit).await
    }

    async fn list_jira_boards(
        &self,
        auth: &AtlassianAuthContext,
        project_key: &str,
    ) -> Result<Vec<JiraBoardSummary>, String> {
        list_jira_boards(self, auth, project_key).await
    }

    async fn get_jira_board_configuration(
        &self,
        auth: &AtlassianAuthContext,
        board_id: &str,
    ) -> Result<JiraBoardConfiguration, String> {
        get_jira_board_configuration(self, auth, board_id).await
    }

    async fn list_jira_active_sprints(
        &self,
        auth: &AtlassianAuthContext,
        board_id: &str,
    ) -> Result<Vec<JiraSprintSummary>, String> {
        list_jira_active_sprints(self, auth, board_id).await
    }

    async fn list_jira_comments(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        start_at: usize,
        max_results: usize,
    ) -> Result<JiraCommentsPage, String> {
        list_jira_comments(self, auth, issue_key, start_at, max_results).await
    }

    async fn list_confluence_spaces(
        &self,
        auth: &AtlassianAuthContext,
        limit: usize,
    ) -> Result<Vec<ConfluenceSpaceSummary>, String> {
        list_confluence_spaces(self, auth, limit).await
    }

    async fn search_jira_users(
        &self,
        auth: &AtlassianAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<JiraUserSummary>, String> {
        search_jira_users(self, auth, query, limit).await
    }

    async fn create_jira_issue(
        &self,
        auth: &AtlassianAuthContext,
        request: &JiraIssueCreateRequest,
    ) -> Result<JiraIssueCreated, AtlassianApiError> {
        atlassian_mcp_client::create_jira_issue(self, auth, request).await
    }

    async fn update_jira_issue(
        &self,
        auth: &AtlassianAuthContext,
        issue_key: &str,
        request: &JiraIssueUpdateRequest,
    ) -> Result<(), AtlassianApiError> {
        atlassian_mcp_client::update_jira_issue(self, auth, issue_key, request).await
    }

    async fn confluence_get_page(
        &self,
        auth: &AtlassianAuthContext,
        page_id: &str,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        atlassian_mcp_client::confluence_get_page(self, auth, page_id).await
    }

    async fn confluence_create_page(
        &self,
        auth: &AtlassianAuthContext,
        request: &ConfluencePageCreateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        atlassian_mcp_client::confluence_create_page(self, auth, request).await
    }

    async fn confluence_update_page(
        &self,
        auth: &AtlassianAuthContext,
        page_id: &str,
        request: &ConfluencePageUpdateRequest,
    ) -> Result<ConfluencePageContent, AtlassianApiError> {
        atlassian_mcp_client::confluence_update_page(self, auth, page_id, request).await
    }

    async fn raw_api_request(
        &self,
        auth: &AtlassianAuthContext,
        method: AtlassianRawMethod,
        kind: AtlassianResourceKind,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, AtlassianApiError> {
        atlassian_mcp_client::raw_api_request(self, auth, method, kind, path, body).await
    }

    async fn exchange_oauth_code(
        &self,
        client_id: &str,
        client_secret: &str,
        code: &str,
        redirect_uri: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        oauth_token_request(
            self,
            serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": client_id,
                "client_secret": client_secret,
                "code": code,
                "redirect_uri": redirect_uri,
            }),
        )
        .await
    }

    async fn refresh_oauth_token(
        &self,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> Result<AtlassianOAuthTokenResponse, String> {
        oauth_token_request(
            self,
            serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": client_id,
                "client_secret": client_secret,
                "refresh_token": refresh_token,
            }),
        )
        .await
    }

    async fn oauth_accessible_resources(
        &self,
        access_token: &str,
    ) -> Result<Vec<AtlassianOAuthResource>, String> {
        let value = self
            .request_json(
                Method::GET,
                "https://api.atlassian.com/oauth/token/accessible-resources".to_string(),
                RequestAuth::Bearer(access_token),
                None,
            )
            .await?;
        Ok(value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|resource| {
                Some(AtlassianOAuthResource {
                    id: resource.get("id").and_then(Value::as_str)?.to_string(),
                    url: resource.get("url").and_then(Value::as_str)?.to_string(),
                    scopes: resource
                        .get("scopes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                })
            })
            .collect())
    }
}

pub(crate) async fn search_jira<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, String> {
    let mut results = Vec::new();
    if let Some(key) = jira_issue_key_query(query) {
        if let Ok(summary) = fetch_jira_summary_by_key(client, auth, &key).await {
            push_unique_resource(&mut results, summary, limit);
        }
    }

    if results.len() < limit {
        match search_jira_with_jql(client, auth, query, limit).await {
            Ok(resources) => {
                for resource in resources {
                    push_unique_resource(&mut results, resource, limit);
                }
            }
            Err(error) if results.is_empty() => {
                return search_jira_picker(client, auth, query, limit)
                    .await
                    .map_err(|_| error);
            }
            Err(_) => {}
        }
    }

    if results.len() < limit {
        if let Ok(resources) = search_jira_picker(client, auth, query, limit).await {
            for resource in resources {
                push_unique_resource(&mut results, resource, limit);
            }
        }
    }

    Ok(results)
}

async fn search_jira_with_jql<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, String> {
    let Some(jql) = build_jira_search_jql(query) else {
        return Ok(Vec::new());
    };
    search_jira_with_jql_string(client, auth, &jql, limit)
        .await
        .map_err(String::from)
}

/// Raw JQL pass-through (gap G1): the caller's string reaches the request
/// body byte-identical, with no smart-mode rewriting. Errors preserve the
/// Atlassian HTTP status so malformed JQL surfaces as a real 400, not a
/// flattened string.
pub(crate) async fn search_jira_raw<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    jql: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, AtlassianApiError> {
    let jql = jql.trim();
    if jql.is_empty() {
        return Err(AtlassianApiError::transport(
            "Raw JQL query must not be blank",
        ));
    }
    search_jira_with_jql_string(client, auth, jql, limit).await
}

async fn search_jira_with_jql_string<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    jql: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, AtlassianApiError> {
    let value = client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                "/rest/api/3/search/jql",
            ),
            request_auth(auth),
            Some(serde_json::json!({
                "jql": jql,
                "fields": ["summary", "status", "issuetype", "assignee", "updated"],
                "maxResults": limit,
            })),
        )
        .await?;
    let site_url = &auth.site_url;
    Ok(value
        .get("issues")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|issue| jira_summary_from_issue_value(issue, site_url))
        .take(limit)
        .collect())
}

async fn search_jira_picker<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, String> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!("/rest/api/3/issue/picker?query={}", percent_encode(query)),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let site_url = &auth.site_url;
    Ok(value
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|section| section.get("issues").and_then(Value::as_array))
        .flatten()
        .filter_map(|issue| jira_summary_from_picker_issue(issue, site_url))
        .take(limit)
        .collect())
}

async fn fetch_jira_summary_by_key<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    key: &str,
) -> Result<AtlassianResourceSummary, String> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}?fields=summary,status",
                    percent_encode_path_segment(key)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    jira_summary_from_issue_value(&value, &auth.site_url)
        .ok_or_else(|| "Atlassian Jira issue response was missing issue details".to_string())
}

pub(crate) fn build_jira_search_jql(query: &str) -> Option<String> {
    let phrase = normalize_search_phrase(query);
    if phrase.is_empty() {
        return None;
    }
    if let Some(key) = jira_issue_key_query(&phrase) {
        return Some(format!("issuekey = {key} ORDER BY updated DESC"));
    }
    Some(format!(
        "text ~ \"{}*\" ORDER BY updated DESC",
        escape_search_string(&phrase)
    ))
}

fn jira_issue_key_query(query: &str) -> Option<String> {
    let value = query.trim();
    let (project, number) = value.split_once('-')?;
    let mut project_chars = project.chars();
    let first_project_char = project_chars.next()?;
    if project.is_empty()
        || number.is_empty()
        || !first_project_char.is_ascii_alphabetic()
        || !project_chars.all(|ch| ch.is_ascii_alphanumeric())
        || !number.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{}-{number}", project.to_ascii_uppercase()))
}

fn jira_summary_from_picker_issue(
    issue: &Value,
    site_url: &str,
) -> Option<AtlassianResourceSummary> {
    let key = issue.get("key").and_then(Value::as_str)?;
    let title = issue
        .get("summaryText")
        .or_else(|| issue.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();
    Some(jira_summary(key, title, site_url))
}

fn jira_summary_from_issue_value(
    issue: &Value,
    site_url: &str,
) -> Option<AtlassianResourceSummary> {
    let key = issue.get("key").and_then(Value::as_str)?;
    let fields = issue.get("fields");
    let title = fields
        .and_then(|fields| fields.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or(key)
        .to_string();
    let mut summary = jira_summary(key, title, site_url);
    summary.status = fields
        .and_then(|fields| fields.get("status"))
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    summary.issue_type = fields
        .and_then(|fields| fields.get("issuetype"))
        .and_then(|issue_type| issue_type.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    summary.assignee = fields.and_then(|fields| jira_user_display_name(fields.get("assignee")));
    summary.updated_at = fields
        .and_then(|fields| fields.get("updated"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(summary)
}

fn jira_summary(key: &str, title: String, site_url: &str) -> AtlassianResourceSummary {
    AtlassianResourceSummary {
        kind: AtlassianResourceKind::Jira,
        id: key.to_string(),
        key: Some(key.to_string()),
        title,
        url: Some(format!("{site_url}/browse/{key}")),
        excerpt: None,
        status: None,
        issue_type: None,
        assignee: None,
        updated_at: None,
    }
}

async fn oauth_token_request<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    body: Value,
) -> Result<AtlassianOAuthTokenResponse, String> {
    let value = client
        .request_json(
            Method::POST,
            "https://auth.atlassian.com/oauth/token".to_string(),
            RequestAuth::None,
            Some(body),
        )
        .await?;
    let access_token = value
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| "Atlassian OAuth response did not include an access token".to_string())?
        .to_string();
    Ok(AtlassianOAuthTokenResponse {
        access_token,
        refresh_token: value
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_string),
        expires_in: value.get("expires_in").and_then(Value::as_i64),
        scope: value
            .get("scope")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

pub(crate) async fn search_confluence<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, String> {
    let mut results = Vec::new();
    if let Some(page_id) = confluence_page_id_query(query) {
        if let Ok(summary) = fetch_confluence_summary_by_id(client, auth, page_id).await {
            push_unique_resource(&mut results, summary, limit);
        }
    }
    if results.len() >= limit {
        return Ok(results);
    }
    let cql = build_confluence_search_cql(query);
    let value = match client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/rest/api/search?cql={}&limit={limit}",
                    percent_encode(&cql)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await
    {
        Ok(value) => value,
        Err(_) if !results.is_empty() => return Ok(results),
        Err(error) => return Err(error.into()),
    };
    let site_url = &auth.site_url;
    for resource in value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| confluence_summary_from_search_result(result, site_url))
    {
        push_unique_resource(&mut results, resource, limit);
    }
    Ok(results)
}

/// Raw CQL pass-through (gap G1): the caller's query string is submitted
/// unmodified to the same v1 search endpoint, skipping the smart-mode page-id
/// short-circuit and dedup merge. Errors preserve the Atlassian HTTP status.
pub(crate) async fn search_confluence_raw<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    cql: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, AtlassianApiError> {
    let cql = cql.trim();
    if cql.is_empty() {
        return Err(AtlassianApiError::transport(
            "Raw CQL query must not be blank",
        ));
    }
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/rest/api/search?cql={}&limit={limit}",
                    percent_encode(cql)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let site_url = &auth.site_url;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| confluence_summary_from_search_result(result, site_url))
        .take(limit)
        .collect())
}

async fn fetch_confluence_summary_by_id<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    page_id: &str,
) -> Result<AtlassianResourceSummary, String> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}",
                    percent_encode_path_segment(page_id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    confluence_summary_from_v2_page(&value, &auth.site_url)
        .ok_or_else(|| "Atlassian Confluence page response was missing page details".to_string())
}

pub(crate) fn build_confluence_search_cql(query: &str) -> String {
    let phrase = normalize_search_phrase(query);
    let escaped = escape_search_string(&phrase);
    let id_clause = confluence_page_id_query(&phrase)
        .map(|page_id| format!("id = {page_id} OR "))
        .unwrap_or_default();
    format!("type=page AND ({id_clause}title ~ \"{escaped}*\" OR text ~ \"{escaped}*\")")
}

pub(crate) fn confluence_page_id_query(query: &str) -> Option<&str> {
    let value = query.trim();
    if value.is_empty() || !value.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(value)
}

fn confluence_summary_from_search_result(
    result: &Value,
    site_url: &str,
) -> Option<AtlassianResourceSummary> {
    let content = result.get("content")?;
    let mut summary = confluence_summary_from_content(content, site_url)?;
    summary.excerpt = result
        .get("excerpt")
        .and_then(Value::as_str)
        .map(strip_html);
    Some(summary)
}

fn confluence_summary_from_v2_page(
    page: &Value,
    site_url: &str,
) -> Option<AtlassianResourceSummary> {
    confluence_summary_from_content(page, site_url)
}

fn confluence_summary_from_content(
    content: &Value,
    site_url: &str,
) -> Option<AtlassianResourceSummary> {
    let id = content.get("id").and_then(Value::as_str)?;
    let title = content.get("title").and_then(Value::as_str).unwrap_or(id);
    let webui = content
        .get("_links")
        .and_then(|links| links.get("webui"))
        .and_then(Value::as_str);
    // Not exposed by an unexpanded CQL search or the plain v2 page GET today;
    // populated only when Atlassian includes a version timestamp on the
    // response, otherwise stays None.
    let updated_at = content
        .get("version")
        .and_then(|version| version.get("when").or_else(|| version.get("createdAt")))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(AtlassianResourceSummary {
        kind: AtlassianResourceKind::Confluence,
        id: id.to_string(),
        key: None,
        title: title.to_string(),
        url: webui.map(|path| format!("{site_url}/wiki{path}")),
        excerpt: None,
        status: None,
        issue_type: None,
        assignee: None,
        updated_at,
    })
}

pub(crate) async fn fetch_jira<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    reference: &ComposerIntegrationReference,
    acceptance_criteria_field_ids: &[String],
) -> Result<AtlassianResourceContent, String> {
    let key = reference.key.as_deref().unwrap_or(&reference.id);
    const JIRA_BASE_ISSUE_FIELDS: [&str; 13] = [
        "summary",
        "status",
        "description",
        "assignee",
        "reporter",
        "updated",
        "comment",
        "attachment",
        "issuetype",
        "labels",
        "priority",
        "parent",
        "subtasks",
    ];
    let requested_fields = JIRA_BASE_ISSUE_FIELDS
        .iter()
        .map(|field| percent_encode(field))
        .chain(
            acceptance_criteria_field_ids
                .iter()
                .map(|field| percent_encode(field)),
        )
        .collect::<Vec<_>>()
        .join(",");
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}?fields={requested_fields}",
                    percent_encode_path_segment(key)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let site_url = &auth.site_url;
    let fields = value.get("fields").unwrap_or(&Value::Null);
    let title = fields
        .get("summary")
        .and_then(Value::as_str)
        .or(reference.title.as_deref())
        .unwrap_or(key)
        .to_string();
    let status = fields
        .get("status")
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let assignee = jira_user_display_name(fields.get("assignee"));
    let reporter = jira_user_display_name(fields.get("reporter"));
    let updated_at_remote = fields
        .get("updated")
        .and_then(Value::as_str)
        .map(str::to_string);
    let issue_type = fields
        .get("issuetype")
        .and_then(|issue_type| issue_type.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let labels = fields
        .get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let priority = fields
        .get("priority")
        .and_then(|priority| priority.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let description = fields
        .get("description")
        .filter(|value| !value.is_null())
        .map(jira_rich_text);
    let description_markdown = description
        .as_ref()
        .map(|content| content.markdown.clone())
        .filter(|value| !value.trim().is_empty());
    let description_text = description
        .as_ref()
        .map(|content| content.text.clone())
        .filter(|value| !value.trim().is_empty());
    let acceptance_criteria =
        acceptance_criteria_from_fields(fields, acceptance_criteria_field_ids);
    let acceptance_criteria_markdown = acceptance_criteria
        .as_ref()
        .map(|value| value.markdown.clone())
        .or_else(|| {
            description_markdown
                .as_deref()
                .and_then(extract_acceptance_criteria)
        });
    let acceptance_criteria_text = acceptance_criteria
        .as_ref()
        .map(|value| value.text.clone())
        .or_else(|| {
            description_text
                .as_deref()
                .and_then(extract_acceptance_criteria)
        });
    let comment_field = fields.get("comment");
    let comment_total = comment_field
        .and_then(|comment| comment.get("total"))
        .and_then(Value::as_u64)
        .map(|total| total as usize);
    let comments = comment_field
        .and_then(|comment| comment.get("comments"))
        .and_then(Value::as_array)
        .map(|comments| {
            comments
                .iter()
                .rev()
                .take(10)
                .rev()
                .filter_map(jira_comment_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let attachments = fields
        .get("attachment")
        .and_then(Value::as_array)
        .map(|attachments| {
            attachments
                .iter()
                .filter_map(jira_attachment_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let parent_field = fields.get("parent");
    let parent_key = parent_field
        .and_then(|parent| parent.get("key"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let parent_summary = parent_field
        .and_then(|parent| parent.get("fields"))
        .and_then(|fields| fields.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let subtasks = fields
        .get("subtasks")
        .and_then(Value::as_array)
        .map(|subtasks| {
            subtasks
                .iter()
                .filter_map(jira_child_issue_from_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut body = Vec::new();
    push_field(&mut body, "Key", key);
    push_field(&mut body, "Summary", &title);
    if let Some(status) = status.as_deref() {
        push_field(&mut body, "Status", status);
    }
    if let Some(updated) = updated_at_remote.as_deref() {
        push_field(&mut body, "Updated", updated);
    }
    if let Some(issue_type) = issue_type.as_deref() {
        push_field(&mut body, "Type", issue_type);
    }
    if let Some(assignee) = assignee.as_deref() {
        push_field(&mut body, "Assignee", assignee);
    }
    if let Some(reporter) = reporter.as_deref() {
        push_field(&mut body, "Reporter", reporter);
    }
    if let Some(priority) = priority.as_deref() {
        push_field(&mut body, "Priority", priority);
    }
    if !labels.is_empty() {
        push_field(&mut body, "Labels", &labels.join(", "));
    }
    if let Some(parent_key) = parent_key.as_deref() {
        let parent_line = match parent_summary.as_deref() {
            Some(summary) => format!("Parent: {parent_key} — {summary}"),
            None => format!("Parent: {parent_key}"),
        };
        body.push(parent_line);
    }
    if !subtasks.is_empty() {
        body.push(format!("Subtasks ({}):", subtasks.len()));
        for subtask in &subtasks {
            let status = subtask.status.as_deref().unwrap_or("Unknown status");
            body.push(format!(
                "- {} — {} ({status})",
                subtask.key, subtask.summary
            ));
        }
    }
    if let Some(acceptance_criteria) = acceptance_criteria_markdown
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        body.push("Acceptance Criteria:".to_string());
        body.push(acceptance_criteria.to_string());
    }
    if let Some(description) = description_markdown.as_deref() {
        body.push("Description:".to_string());
        body.push(description.to_string());
    }
    if !attachments.is_empty() {
        body.push(format!("Attachments ({}):", attachments.len()));
        for attachment in &attachments {
            let mime_type = attachment.mime_type.as_deref().unwrap_or("unknown type");
            let size = attachment
                .size
                .map(human_readable_size)
                .unwrap_or_else(|| "unknown size".to_string());
            body.push(format!("- {} ({mime_type}, {size})", attachment.filename));
        }
        body.push("(readable via list_ticket_attachments / fetch_ticket_attachment)".to_string());
    }
    const JIRA_PROMPT_COMMENT_LIMIT: usize = 5;
    for comment in comments.iter().rev().take(JIRA_PROMPT_COMMENT_LIMIT).rev() {
        let author = comment.author.as_deref().unwrap_or("Jira user");
        let timestamp = comment
            .updated_at
            .as_deref()
            .or(comment.created_at.as_deref())
            .unwrap_or("unknown date");
        body.push(format!("Comment by {author} ({timestamp}):"));
        body.push(comment.body_markdown.clone());
    }
    let total_comments = comment_total.unwrap_or(comments.len());
    if total_comments > JIRA_PROMPT_COMMENT_LIMIT {
        body.push(format!(
            "({total_comments} total comments; showing latest {JIRA_PROMPT_COMMENT_LIMIT} — jira_list_comments for more)"
        ));
    }
    let children = if issue_type
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case("epic"))
    {
        match fetch_jira_epic_children(client, auth, key).await {
            Ok((children, total)) => {
                body.push(format!(
                    "Child issues ({} shown of {total}):",
                    children.len()
                ));
                for child in &children {
                    let status = child.status.as_deref().unwrap_or("Unknown status");
                    body.push(format!("- {} — {} ({status})", child.key, child.summary));
                }
                children
            }
            Err(err) => {
                tracing::warn!(
                    issue_key = key,
                    error = %err,
                    "Jira epic child-issue lookup failed"
                );
                body.push("Child issues: unavailable".to_string());
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    Ok(AtlassianResourceContent {
        kind: AtlassianResourceKind::Jira,
        id: key.to_string(),
        key: Some(key.to_string()),
        title,
        url: Some(format!("{site_url}/browse/{key}")),
        body: body.join("\n"),
        status,
        assignee,
        reporter,
        updated_at_remote,
        description_markdown,
        description_text,
        acceptance_criteria_markdown,
        acceptance_criteria_text,
        comments,
        attachments,
        issue_type,
        labels,
        priority,
        parent_key,
        children,
    })
}

/// Cap on the epic-children secondary lookup (gap G4/Phase 3.1): a JQL page
/// large enough for prompt context without unbounded growth.
const EPIC_CHILDREN_LIMIT: usize = 25;

/// One extra capped JQL call for Epic issues only, rendering direct children
/// (`parent = KEY`). Failure here must never fail the whole Jira expansion —
/// callers render a "Child issues: unavailable" line instead.
async fn fetch_jira_epic_children<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    key: &str,
) -> Result<(Vec<AtlassianJiraChildIssue>, usize), AtlassianApiError> {
    let value = client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                "/rest/api/3/search/jql",
            ),
            request_auth(auth),
            Some(serde_json::json!({
                "jql": format!("parent = {key} ORDER BY rank"),
                "fields": ["summary", "status", "issuetype"],
                "maxResults": EPIC_CHILDREN_LIMIT,
            })),
        )
        .await?;
    let issues = value
        .get("issues")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .map(|total| total as usize)
        .unwrap_or(issues.len());
    let children = issues
        .iter()
        .filter_map(jira_child_issue_from_value)
        .take(EPIC_CHILDREN_LIMIT)
        .collect::<Vec<_>>();
    Ok((children, total))
}

fn jira_child_issue_from_value(value: &Value) -> Option<AtlassianJiraChildIssue> {
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let issue_fields = value.get("fields");
    let summary = issue_fields
        .and_then(|fields| fields.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("(no summary)")
        .to_string();
    let status = issue_fields
        .and_then(|fields| fields.get("status"))
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let issue_type = issue_fields
        .and_then(|fields| fields.get("issuetype"))
        .and_then(|issue_type| issue_type.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    Some(AtlassianJiraChildIssue {
        key,
        summary,
        status,
        issue_type,
    })
}

/// Render a byte count as a short human-readable size (`"240 KB"`), used only
/// for the rendered Jira attachments section — never for byte-accurate math.
fn human_readable_size(bytes: i64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes <= 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit_index = 0;
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    format!("{value:.0} {}", UNITS[unit_index])
}

pub(crate) async fn assign_jira_issue_to_current_user<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
) -> Result<(), String> {
    let issue_key = issue_key.trim();
    if issue_key.is_empty() {
        return Err("Jira issue key is required".to_string());
    }
    let myself = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                "/rest/api/3/myself",
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let account_id = myself
        .get("accountId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Atlassian current user response did not include accountId".to_string())?;
    client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/assignee",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "accountId": account_id })),
        )
        .await?;
    Ok(())
}

pub(crate) async fn clear_jira_issue_assignee<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
) -> Result<(), String> {
    let issue_key = issue_key.trim();
    if issue_key.is_empty() {
        return Err("Jira issue key is required".to_string());
    }
    client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/assignee",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "accountId": null })),
        )
        .await?;
    Ok(())
}

/// Assign a Jira issue to a specific account. Higher precedence than
/// `assign_to_me` in `jira_assign_issue`'s resolution order.
pub(crate) async fn assign_jira_issue_to_account<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    account_id: &str,
) -> Result<(), String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    let account_id = required_trimmed(account_id, "Jira accountId is required")?;
    client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/assignee",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "accountId": account_id })),
        )
        .await?;
    Ok(())
}

/// Upper bound on `jira_search_users` results, mirroring the MCP tool schema.
const JIRA_USER_SEARCH_LIMIT: usize = 20;

/// Bounded Jira user search, used to resolve an `accountId` for
/// `jira_assign_issue`.
pub(crate) async fn search_jira_users<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<JiraUserSummary>, String> {
    let query = required_trimmed(query, "Jira user search query is required")?;
    let limit = limit.clamp(1, JIRA_USER_SEARCH_LIMIT);
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/user/search?query={}&maxResults={limit}",
                    percent_encode(query)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(jira_user_summary_from_value)
        .take(limit)
        .collect())
}

fn jira_user_summary_from_value(value: &Value) -> Option<JiraUserSummary> {
    let account_id = value
        .get("accountId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let display_name = value
        .get("displayName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&account_id)
        .to_string();
    Some(JiraUserSummary {
        account_id,
        display_name,
    })
}

/// Lists comments on a Jira issue with the provider's true total, so
/// `fetch_jira`'s inline comment-count hint can point agents here for the
/// rest. Reuses [`jira_comment_from_value`], the same ADF→markdown comment
/// parser `fetch_jira` uses.
pub(crate) async fn list_jira_comments<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    start_at: usize,
    max_results: usize,
) -> Result<JiraCommentsPage, String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/comment?startAt={start_at}&maxResults={max_results}&orderBy=-created",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let total = value
        .get("total")
        .and_then(Value::as_u64)
        .map(|total| total as usize)
        .unwrap_or(0);
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .map(|comments| comments.iter().filter_map(jira_comment_from_value).collect())
        .unwrap_or_default();
    Ok(JiraCommentsPage { comments, total })
}

/// Lists Confluence spaces (v2 API), used to unblock `confluence_create_page`'s
/// otherwise-unguessable `spaceId`.
pub(crate) async fn list_confluence_spaces<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    limit: usize,
) -> Result<Vec<ConfluenceSpaceSummary>, String> {
    let limit = limit.clamp(1, 250);
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!("/wiki/api/v2/spaces?limit={limit}"),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .map(|spaces| {
            spaces
                .iter()
                .filter_map(confluence_space_summary_from_value)
                .collect()
        })
        .unwrap_or_default())
}

fn confluence_space_summary_from_value(value: &Value) -> Option<ConfluenceSpaceSummary> {
    let id = value.get("id").and_then(|id| {
        id.as_str()
            .map(str::to_string)
            .or_else(|| id.as_i64().map(|id| id.to_string()))
    })?;
    let key = value
        .get("key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(ConfluenceSpaceSummary { id, key, name })
}

pub(crate) async fn set_jira_issue_labels<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    labels: Vec<String>,
) -> Result<(), String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    client
        .request_json(
            Method::PUT,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "fields": { "labels": labels } })),
        )
        .await?;
    Ok(())
}

pub(crate) async fn list_jira_issue_transitions<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
) -> Result<Vec<AtlassianJiraTransition>, String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/transitions",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(value
        .get("transitions")
        .and_then(Value::as_array)
        .map(|transitions| {
            transitions
                .iter()
                .filter_map(jira_transition_from_value)
                .collect()
        })
        .unwrap_or_default())
}

pub(crate) async fn transition_jira_issue<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    transition_id: &str,
) -> Result<(), String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    let transition_id = required_trimmed(transition_id, "Jira transition id is required")?;
    client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/transitions",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "transition": { "id": transition_id } })),
        )
        .await?;
    Ok(())
}

pub(crate) async fn add_jira_comment<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    issue_key: &str,
    body_markdown: &str,
) -> Result<AtlassianJiraComment, String> {
    let issue_key = required_trimmed(issue_key, "Jira issue key is required")?;
    let body_markdown = required_trimmed(body_markdown, "Jira comment body is required")?;
    let value = client
        .request_json(
            Method::POST,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}/comment",
                    percent_encode_path_segment(issue_key)
                ),
            ),
            request_auth(auth),
            Some(serde_json::json!({ "body": jira_comment_adf(body_markdown) })),
        )
        .await?;
    jira_comment_from_value(&value).ok_or_else(|| {
        "Jira comment response did not include a readable created comment".to_string()
    })
}

/// List Jira projects used as ticketing containers, walking Jira's paged
/// project-search response until `limit` is reached or the server reports the
/// final page.
pub(crate) async fn list_jira_projects<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    limit: usize,
) -> Result<Vec<JiraProjectSummary>, String> {
    let limit = limit.max(1);
    let page_size = limit.clamp(1, JIRA_PROJECT_SEARCH_PAGE_SIZE);
    let mut start_at = 0usize;
    let mut projects = Vec::new();

    while projects.len() < limit {
        let value = client
            .request_json(
                Method::GET,
                HyperAtlassianApiClient::resource_url(
                    auth,
                    AtlassianResourceKind::Jira,
                    &format!(
                        "/rest/api/3/project/search?startAt={start_at}&maxResults={page_size}&orderBy=name"
                    ),
                ),
                request_auth(auth),
                None,
            )
            .await?;

        let values = value
            .get("values")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = values.len();
        projects.extend(values.iter().filter_map(jira_project_from_value));
        if projects.len() >= limit {
            projects.truncate(limit);
            break;
        }
        if count == 0 {
            break;
        }
        if count < page_size {
            break;
        }

        let response_start = value
            .get("startAt")
            .and_then(Value::as_u64)
            .map(|value| value as usize)
            .unwrap_or(start_at);
        let total_reached = value
            .get("total")
            .and_then(Value::as_u64)
            .map(|total| response_start + count >= total as usize)
            .unwrap_or(false);
        let is_last = value
            .get("isLast")
            .and_then(Value::as_bool)
            .unwrap_or(total_reached);
        if is_last {
            break;
        }
        start_at = response_start + count;
    }

    Ok(projects)
}

fn jira_project_from_value(project: &Value) -> Option<JiraProjectSummary> {
    let key = project.get("key").and_then(Value::as_str)?.trim();
    if key.is_empty() {
        return None;
    }
    let id = project
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| key.to_string());
    let name = project
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(key)
        .to_string();
    Some(JiraProjectSummary {
        id,
        key: key.to_string(),
        name,
    })
}

/// List the statuses available for a project, flattened across all issue types
/// and deduped by status id. Uses the non-admin `project/{key}/statuses` endpoint
/// (the `/statuses/search` variant needs admin).
pub(crate) async fn list_jira_project_statuses<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    project_key: &str,
) -> Result<Vec<JiraStatusSummary>, String> {
    let project_key = required_trimmed(project_key, "Jira project key is required")?;
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/project/{}/statuses",
                    percent_encode_path_segment(project_key)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    Ok(jira_project_statuses_from_value(&value))
}

/// Flatten the `[ { statuses: [ { id, name, statusCategory } ] } ]` issue-type
/// grouping into a deduped status list (first occurrence of each id wins),
/// mapping each status to a normalized category via [`jira_status_category`].
fn jira_project_statuses_from_value(value: &Value) -> Vec<JiraStatusSummary> {
    let mut seen = std::collections::HashSet::new();
    let mut statuses = Vec::new();
    for issue_type in value.as_array().into_iter().flatten() {
        for status in issue_type
            .get("statuses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(id) = status.get("id").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if id.is_empty() || !seen.insert(id.to_string()) {
                continue;
            }
            let name = status
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(id)
                .to_string();
            statuses.push(JiraStatusSummary {
                category: jira_status_category(status, &name),
                id: id.to_string(),
                name,
            });
        }
    }
    statuses
}

/// Build the JQL used to list a project's issues for kanban browse:
/// `project = "<escaped>" ORDER BY updated DESC`. Status filtering stays
/// client-side (kanban columns), so no status clause is needed for v1.
pub(crate) fn build_jira_project_jql(project_key: &str) -> String {
    format!(
        "project = \"{}\" ORDER BY updated DESC",
        escape_search_string(project_key)
    )
}

/// Fetch a project's issues (single page) with the richer field set needed for
/// kanban columns and the ticket list. Reuses the `/rest/api/3/search/jql`
/// endpoint/body shape from [`search_jira_with_jql`].
pub(crate) async fn search_jira_by_project<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    project_key: &str,
    limit: usize,
) -> Result<Vec<JiraIssueDetail>, String> {
    let project_key = required_trimmed(project_key, "Jira project key is required")?;
    let jql = build_jira_project_jql(project_key);
    let site_url = &auth.site_url;
    let limit = limit.max(1);
    let page_size = limit.clamp(1, JIRA_ISSUE_SEARCH_PAGE_SIZE);
    let mut next_page_token: Option<String> = None;
    let mut issues = Vec::new();

    loop {
        let mut body = serde_json::json!({
            "jql": jql,
            "fields": ["summary", "status", "assignee", "labels", "updated", "priority"],
            "maxResults": page_size,
        });
        if let (Some(token), Value::Object(map)) = (next_page_token.as_ref(), &mut body) {
            map.insert("nextPageToken".to_string(), Value::String(token.clone()));
        }

        let value = client
            .request_json(
                Method::POST,
                HyperAtlassianApiClient::resource_url(
                    auth,
                    AtlassianResourceKind::Jira,
                    "/rest/api/3/search/jql",
                ),
                request_auth(auth),
                Some(body),
            )
            .await?;
        let page_issues = value
            .get("issues")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let count = page_issues.len();
        issues.extend(
            page_issues
                .iter()
                .filter_map(|issue| jira_ticket_from_issue_value(issue, site_url)),
        );
        if issues.len() >= limit {
            issues.truncate(limit);
            break;
        }
        if count == 0 {
            break;
        }

        next_page_token = value
            .get("nextPageToken")
            .and_then(Value::as_str)
            .map(str::to_string);
        if next_page_token.is_none() {
            break;
        }
    }

    Ok(issues)
}

/// Parse a Jira issue payload into a [`JiraIssueDetail`], preserving the
/// status/assignee/labels/updated/priority the kanban + ticket list require
/// (the existing `jira_summary_from_issue_value` is too lossy — it only keeps
/// id/key/title and drops status).
fn jira_ticket_from_issue_value(issue: &Value, site_url: &str) -> Option<JiraIssueDetail> {
    let key = issue.get("key").and_then(Value::as_str)?.trim();
    if key.is_empty() {
        return None;
    }
    let fields = issue.get("fields").unwrap_or(&Value::Null);
    let title = fields
        .get("summary")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(key)
        .to_string();
    let status = fields.get("status");
    let status_id = status
        .and_then(|status| status.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let status_name = status
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let status_category = status
        .map(|status| jira_status_category(status, status_name.as_deref().unwrap_or_default()));
    let assignee = fields.get("assignee").filter(|value| !value.is_null());
    let assignee_name = jira_user_display_name(assignee);
    let assignee_avatar = assignee
        .and_then(|user| user.get("avatarUrls"))
        .and_then(|avatars| avatars.get("48x48").or_else(|| avatars.get("24x24")))
        .and_then(Value::as_str)
        .map(str::to_string);
    let labels = fields
        .get("labels")
        .and_then(Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let updated = fields
        .get("updated")
        .and_then(Value::as_str)
        .map(str::to_string);
    let priority = fields
        .get("priority")
        .and_then(|priority| priority.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Some(JiraIssueDetail {
        key: key.to_string(),
        title,
        status_id,
        status_name,
        status_category,
        assignee_name,
        assignee_avatar,
        labels,
        updated,
        priority,
        url: Some(format!("{site_url}/browse/{key}")),
    })
}

pub(crate) async fn fetch_confluence<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
    reference: &ComposerIntegrationReference,
) -> Result<AtlassianResourceContent, String> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Confluence,
                &format!(
                    "/wiki/api/v2/pages/{}?body-format=storage",
                    percent_encode_path_segment(&reference.id)
                ),
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let site_url = &auth.site_url;
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .or(reference.title.as_deref())
        .unwrap_or(&reference.id)
        .to_string();
    let body = value
        .get("body")
        .and_then(|body| body.get("storage"))
        .and_then(|storage| storage.get("value"))
        .and_then(Value::as_str)
        .map(strip_html)
        .unwrap_or_default();
    let url = value
        .get("_links")
        .and_then(|links| links.get("webui"))
        .and_then(Value::as_str)
        .map(|path| format!("{site_url}/wiki{path}"))
        .or_else(|| reference.url.clone());

    let page_id = &reference.id;
    let comments =
        match confluence_secondary::fetch_confluence_footer_comments(client, auth, page_id).await {
            Ok(comments) => comments,
            Err(error) => {
                tracing::warn!(
                    page_id = %page_id,
                    error = %error,
                    "Confluence footer-comments lookup failed"
                );
                Vec::new()
            }
        };
    let attachments =
        match confluence_secondary::fetch_confluence_attachments(client, auth, page_id).await {
            Ok(attachments) => attachments,
            Err(error) => {
                tracing::warn!(
                    page_id = %page_id,
                    error = %error,
                    "Confluence attachments lookup failed"
                );
                Vec::new()
            }
        };
    let children =
        match confluence_secondary::fetch_confluence_child_pages(client, auth, page_id).await {
            Ok(children) => children,
            Err(error) => {
                tracing::warn!(
                    page_id = %page_id,
                    error = %error,
                    "Confluence child-page lookup failed"
                );
                Vec::new()
            }
        };

    let mut body_lines = vec![body];
    if !attachments.is_empty() {
        body_lines.push(format!("Attachments ({}):", attachments.len()));
        for attachment in &attachments {
            let mime_type = attachment.mime_type.as_deref().unwrap_or("unknown type");
            let size = attachment
                .size
                .map(human_readable_size)
                .unwrap_or_else(|| "unknown size".to_string());
            body_lines.push(format!("- {} ({mime_type}, {size})", attachment.filename));
        }
    }
    const CONFLUENCE_PROMPT_COMMENT_LIMIT: usize = 5;
    for comment in comments
        .iter()
        .rev()
        .take(CONFLUENCE_PROMPT_COMMENT_LIMIT)
        .rev()
    {
        let author = comment.author.as_deref().unwrap_or("Confluence user");
        let timestamp = comment
            .updated_at
            .as_deref()
            .or(comment.created_at.as_deref())
            .unwrap_or("unknown date");
        body_lines.push(format!("Comment by {author} ({timestamp}):"));
        body_lines.push(comment.body_markdown.clone());
    }
    if comments.len() > CONFLUENCE_PROMPT_COMMENT_LIMIT {
        body_lines.push(format!(
            "({} older comments omitted)",
            comments.len() - CONFLUENCE_PROMPT_COMMENT_LIMIT
        ));
    }
    if !children.is_empty() {
        body_lines.push(format!("Child pages ({}):", children.len()));
        for child in &children {
            body_lines.push(format!("- {} (id {})", child.summary, child.key));
        }
    }

    Ok(AtlassianResourceContent {
        kind: AtlassianResourceKind::Confluence,
        id: reference.id.clone(),
        key: None,
        title,
        url,
        body: body_lines.join("\n"),
        status: None,
        assignee: None,
        reporter: None,
        updated_at_remote: None,
        description_markdown: None,
        description_text: None,
        acceptance_criteria_markdown: None,
        acceptance_criteria_text: None,
        comments,
        attachments,
        issue_type: None,
        labels: Vec::new(),
        priority: None,
        parent_key: None,
        children,
    })
}

fn push_field(lines: &mut Vec<String>, label: &str, value: &str) {
    if !value.trim().is_empty() {
        lines.push(format!("{label}: {}", value.trim()));
    }
}

pub(super) fn required_trimmed<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(message.to_string())
    } else {
        Ok(value)
    }
}

pub(super) fn jira_transition_from_value(value: &Value) -> Option<AtlassianJiraTransition> {
    let provider_transition_id = value.get("id")?.as_str()?.trim();
    let name = value.get("name")?.as_str()?.trim();
    let to = value.get("to")?;
    let to_state_id = to.get("id")?.as_str()?.trim();
    let to_state_name = to.get("name").and_then(Value::as_str).unwrap_or(name);
    if provider_transition_id.is_empty() || name.is_empty() || to_state_id.is_empty() {
        return None;
    }
    Some(AtlassianJiraTransition {
        provider_transition_id: provider_transition_id.to_string(),
        to_state_id: to_state_id.to_string(),
        name: name.to_string(),
        category: jira_status_category(to, to_state_name),
    })
}

pub(super) fn jira_status_category(status: &Value, fallback_name: &str) -> String {
    let category = status
        .get("statusCategory")
        .and_then(|category| category.get("key").or_else(|| category.get("name")))
        .and_then(Value::as_str)
        .unwrap_or(fallback_name)
        .to_ascii_lowercase();
    if category.contains("done")
        || category.contains("complete")
        || category.contains("closed")
        || category.contains("resolved")
    {
        "done".to_string()
    } else if category.contains("indeterminate")
        || category.contains("progress")
        || category.contains("review")
        || category.contains("started")
        || category.contains("active")
    {
        "in_progress".to_string()
    } else if category.contains("new")
        || category.contains("todo")
        || category.contains("to do")
        || category.contains("backlog")
    {
        "todo".to_string()
    } else {
        "other".to_string()
    }
}

pub(super) fn jira_comment_adf(body_markdown: &str) -> Value {
    super::adf_markdown_writer::markdown_to_adf(body_markdown)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JiraRichText {
    pub(crate) markdown: String,
    pub(crate) text: String,
}

pub(crate) fn jira_rich_text(value: &Value) -> JiraRichText {
    let markdown = value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| adf_node_to_markdown(value, 0));
    let markdown = normalize_markdown(&markdown);
    JiraRichText {
        text: markdown_to_plain_text(&markdown),
        markdown,
    }
}

fn jira_user_display_name(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|user| user.get("displayName"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn jira_comment_from_value(value: &Value) -> Option<AtlassianJiraComment> {
    let body = value.get("body").map(jira_rich_text)?;
    if body.markdown.trim().is_empty() && body.text.trim().is_empty() {
        return None;
    }
    Some(AtlassianJiraComment {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        author: jira_user_display_name(value.get("author")),
        body_markdown: body.markdown,
        body_text: body.text,
        created_at: value
            .get("created")
            .and_then(Value::as_str)
            .map(str::to_string),
        updated_at: value
            .get("updated")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn jira_attachment_from_value(value: &Value) -> Option<AtlassianJiraAttachment> {
    let filename = value
        .get("filename")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    Some(AtlassianJiraAttachment {
        id: value.get("id").and_then(Value::as_str).map(str::to_string),
        filename,
        mime_type: value
            .get("mimeType")
            .and_then(Value::as_str)
            .map(str::to_string),
        size: value.get("size").and_then(Value::as_i64),
        author: jira_user_display_name(value.get("author")),
        content_url: value
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_string),
        thumbnail_url: value
            .get("thumbnail")
            .and_then(Value::as_str)
            .map(str::to_string),
        created_at: value
            .get("created")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn adf_node_to_markdown(value: &Value, depth: usize) -> String {
    let node_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match node_type {
        "doc" => adf_content_to_markdown(value, depth),
        "paragraph" => adf_inline_content_to_markdown(value),
        "text" => adf_text_node_to_markdown(value),
        "hardBreak" => "\n".to_string(),
        "heading" => {
            let level = value
                .get("attrs")
                .and_then(|attrs| attrs.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(2)
                .clamp(1, 6) as usize;
            format!(
                "{} {}",
                "#".repeat(level),
                adf_inline_content_to_markdown(value).trim()
            )
        }
        "bulletList" => adf_list_to_markdown(value, depth, false),
        "orderedList" => adf_list_to_markdown(value, depth, true),
        "listItem" => adf_content_to_markdown(value, depth),
        "blockquote" => adf_content_to_markdown(value, depth)
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n"),
        "codeBlock" => format!(
            "```\n{}\n```",
            adf_content_to_markdown(value, depth).trim_matches('\n')
        ),
        "inlineCard" | "blockCard" => value
            .get("attrs")
            .and_then(|attrs| attrs.get("url"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "mention" => value
            .get("attrs")
            .and_then(|attrs| {
                attrs
                    .get("text")
                    .or_else(|| attrs.get("displayName"))
                    .and_then(Value::as_str)
            })
            .unwrap_or_default()
            .to_string(),
        "emoji" => value
            .get("attrs")
            .and_then(|attrs| attrs.get("shortName"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => {
            let rendered = adf_content_to_markdown(value, depth);
            if rendered.trim().is_empty() {
                render_jsonish(value)
            } else {
                rendered
            }
        }
    }
}

fn adf_content_to_markdown(value: &Value, depth: usize) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .map(|child| adf_node_to_markdown(child, depth))
                .filter(|child| !child.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn adf_inline_content_to_markdown(value: &Value) -> String {
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|content| {
            content
                .iter()
                .map(|child| adf_node_to_markdown(child, 0))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

fn adf_text_node_to_markdown(value: &Value) -> String {
    let mut text = value
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(marks) = value.get("marks").and_then(Value::as_array) {
        for mark in marks {
            match mark.get("type").and_then(Value::as_str) {
                Some("code") => text = format!("`{text}`"),
                Some("strong") => text = format!("**{text}**"),
                Some("em") => text = format!("*{text}*"),
                Some("link") => {
                    if let Some(href) = mark
                        .get("attrs")
                        .and_then(|attrs| attrs.get("href"))
                        .and_then(Value::as_str)
                    {
                        text = format!("[{text}]({href})");
                    }
                }
                _ => {}
            }
        }
    }
    text
}

fn adf_list_to_markdown(value: &Value, depth: usize, ordered: bool) -> String {
    let indent = "  ".repeat(depth);
    value
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let bullet = if ordered {
                        format!("{}.", index + 1)
                    } else {
                        "-".to_string()
                    };
                    let item_body = adf_node_to_markdown(item, depth + 1);
                    let mut lines = item_body.lines();
                    let first = lines.next().unwrap_or_default();
                    let mut rendered = format!("{indent}{bullet} {first}");
                    for line in lines {
                        rendered.push('\n');
                        rendered.push_str(&format!("{indent}  {line}"));
                    }
                    rendered
                })
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn normalize_markdown(value: &str) -> String {
    value
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}

fn markdown_to_plain_text(value: &str) -> String {
    value
        .replace("**", "")
        .replace('`', "")
        .lines()
        .map(|line| {
            line.trim_start()
                .trim_start_matches('#')
                .trim_start_matches("- ")
                .trim_start()
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn extract_acceptance_criteria(value: &str) -> Option<String> {
    let mut collecting = false;
    let mut lines = Vec::new();
    for line in value.lines() {
        let normalized = line
            .trim()
            .trim_start_matches('#')
            .trim()
            .trim_end_matches(':')
            .to_ascii_lowercase();
        if collecting {
            if is_likely_heading(line) && !normalized.contains("acceptance criteria") {
                break;
            }
            lines.push(line.to_string());
        } else if normalized == "acceptance criteria" || normalized == "acceptance criterias" {
            collecting = true;
        }
    }
    let rendered = lines.join("\n").trim().to_string();
    (!rendered.is_empty()).then_some(rendered)
}

fn is_likely_heading(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('#')
        || (trimmed.ends_with(':')
            && trimmed.len() <= 80
            && !trimmed.starts_with('-')
            && !trimmed.chars().any(|ch| ch == '.'))
}

fn render_jsonish(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

pub(crate) fn strip_html(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                output.push(' ');
            }
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_search_phrase(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn escape_search_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn push_unique_resource(
    resources: &mut Vec<AtlassianResourceSummary>,
    resource: AtlassianResourceSummary,
    limit: usize,
) {
    if resources.len() >= limit {
        return;
    }
    if resources
        .iter()
        .any(|existing| existing.kind == resource.kind && existing.id == resource.id)
    {
        return;
    }
    resources.push(resource);
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

pub(crate) fn percent_encode_path_segment(value: &str) -> String {
    percent_encode(value)
}
