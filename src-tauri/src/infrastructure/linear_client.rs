use async_trait::async_trait;
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
    LinearApiClient, LinearAttachment, LinearAuthContext, LinearComment, LinearIssueContent,
    LinearIssueSummary, LinearLabel, LinearProject, LinearUser, LinearWorkflowState,
};
use crate::domain::services::ComposerIntegrationReference;

const LINEAR_GRAPHQL_ENDPOINT: &str = "https://api.linear.app/graphql";
const LINEAR_CONNECTION_PAGE_SIZE: usize = 100;
const LINEAR_LABEL_PAGE_SIZE: usize = 250;

pub struct HyperLinearApiClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    timeout: Duration,
}

impl HyperLinearApiClient {
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

    async fn graphql(
        &self,
        api_token: &str,
        query: &str,
        variables: Value,
    ) -> Result<Value, String> {
        let body = serde_json::json!({
            "query": query,
            "variables": variables,
        });
        let body_bytes = serde_json::to_vec(&body).map_err(|error| error.to_string())?;
        let request = Request::builder()
            .method(Method::POST)
            .uri(LINEAR_GRAPHQL_ENDPOINT)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Authorization", api_token)
            .body(Full::new(Bytes::from(body_bytes)))
            .map_err(|error| format!("Failed to build Linear request: {error}"))?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| "Linear request timed out".to_string())?
            .map_err(|error| format!("Linear request failed: {error}"))?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| format!("Failed to read Linear response: {error}"))?
            .to_bytes();
        if !status.is_success() {
            return Err(render_http_error(status.as_u16(), &bytes));
        }
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Linear response: {error}"))?;
        if let Some(errors) = value.get("errors").and_then(|value| value.as_array()) {
            if !errors.is_empty() {
                return Err(format!(
                    "Linear GraphQL error: {}",
                    render_graphql_errors(errors)
                ));
            }
        }
        Ok(value.get("data").cloned().unwrap_or(Value::Null))
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

fn render_graphql_errors(errors: &[Value]) -> String {
    errors
        .iter()
        .filter_map(|error| error.get("message").and_then(|message| message.as_str()))
        .collect::<Vec<_>>()
        .join("; ")
}

#[async_trait]
impl LinearApiClient for HyperLinearApiClient {
    async fn validate(&self, auth: &LinearAuthContext) -> Result<(), String> {
        let data = self
            .graphql(
                &auth.api_token,
                "query LinearViewer { viewer { id name } }",
                Value::Object(Default::default()),
            )
            .await?;
        validate_viewer_data(&data)
    }

    async fn search_issues(
        &self,
        auth: &LinearAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        let limit = limit.max(1);
        let page_size = limit.clamp(1, LINEAR_CONNECTION_PAGE_SIZE);
        let search_term = query.trim();
        let connection_key = if search_term.is_empty() {
            "issues"
        } else {
            "searchIssues"
        };
        let mut after: Option<String> = None;
        let mut issues = Vec::new();

        while issues.len() < limit {
            let (query_document, variables) = if search_term.is_empty() {
                (
                    linear_recent_issues_query(),
                    serde_json::json!({
                        "first": page_size as i64,
                        "after": after,
                    }),
                )
            } else {
                (
                    linear_issue_search_query(),
                    serde_json::json!({
                        "term": search_term,
                        "first": page_size as i64,
                        "after": after,
                    }),
                )
            };
            let data = self
                .graphql(&auth.api_token, query_document, variables)
                .await?;
            let page = search_issue_summaries_from_data(&data)?;
            let count = page.len();
            issues.extend(page);
            if issues.len() >= limit {
                issues.truncate(limit);
                break;
            }
            if count == 0 {
                break;
            }
            let page_info = page_info_from_connection(&data, connection_key);
            if !page_info.has_next_page {
                break;
            }
            after = page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }

        Ok(issues)
    }

    async fn fetch_issue(
        &self,
        auth: &LinearAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<LinearIssueContent, String> {
        let data = self
            .graphql(
                &auth.api_token,
                r#"
                query RalphXLinearIssue($id: String!) {
                  issue(id: $id) {
                    id
                    identifier
                    title
                    url
                    description
                    updatedAt
                    state {
                      name
                    }
                    assignee {
                      name
                    }
                    creator {
                      name
                    }
                    labels {
                      nodes {
                        name
                      }
                    }
                    project {
                      name
                    }
                    attachments(first: 50) {
                      nodes {
                        id
                        title
                        subtitle
                        url
                      }
                    }
                    comments(first: 50) {
                      nodes {
                        id
                        body
                        createdAt
                        updatedAt
                        user {
                          id
                          name
                        }
                      }
                    }
                  }
                }
                "#,
                serde_json::json!({
                    "id": reference.id,
                }),
            )
            .await?;
        issue_content_from_data(&data, &reference.id)
    }

    async fn list_workflow_states(
        &self,
        auth: &LinearAuthContext,
        team_id: Option<&str>,
    ) -> Result<Vec<LinearWorkflowState>, String> {
        let mut after: Option<String> = None;
        let mut states = Vec::new();
        loop {
            let (query, variables) = linear_workflow_states_query(team_id, after.as_deref());
            let data = self.graphql(&auth.api_token, query, variables).await?;
            let page = workflow_states_from_data(&data)?;
            let count = page.len();
            states.extend(page);
            if count == 0 {
                break;
            }
            let page_info = page_info_from_connection(&data, "workflowStates");
            if !page_info.has_next_page {
                break;
            }
            after = page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }
        Ok(states)
    }

    async fn current_user(&self, auth: &LinearAuthContext) -> Result<LinearUser, String> {
        let data = self
            .graphql(
                &auth.api_token,
                "query RalphXLinearCurrentUser { viewer { id name } }",
                Value::Object(Default::default()),
            )
            .await?;
        current_user_from_data(&data)
    }

    async fn update_issue_state(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        state_id: &str,
    ) -> Result<(), String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let state_id = required_trimmed(state_id, "Linear workflow state id is required")?;
        let data = self
            .graphql(
                &auth.api_token,
                linear_issue_update_mutation(),
                serde_json::json!({
                    "id": issue_id,
                    "input": { "stateId": state_id },
                }),
            )
            .await?;
        mutation_success(&data, "issueUpdate")
    }

    async fn assign_issue_to_current_user(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<LinearUser, String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let user = self.current_user(auth).await?;
        let data = self
            .graphql(
                &auth.api_token,
                linear_issue_update_mutation(),
                serde_json::json!({
                    "id": issue_id,
                    "input": { "assigneeId": user.id },
                }),
            )
            .await?;
        mutation_success(&data, "issueUpdate")?;
        Ok(user)
    }

    async fn clear_issue_assignee(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<(), String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let data = self
            .graphql(
                &auth.api_token,
                linear_issue_update_mutation(),
                serde_json::json!({
                    "id": issue_id,
                    "input": { "assigneeId": null },
                }),
            )
            .await?;
        mutation_success(&data, "issueUpdate")
    }

    async fn create_comment(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        body_markdown: &str,
    ) -> Result<LinearComment, String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let body_markdown = required_trimmed(body_markdown, "Linear comment body is required")?;
        let data = self
            .graphql(
                &auth.api_token,
                linear_comment_create_mutation(),
                serde_json::json!({
                    "input": {
                        "issueId": issue_id,
                        "body": body_markdown,
                    },
                }),
            )
            .await?;
        comment_from_create_data(&data)
    }

    async fn list_projects(
        &self,
        auth: &LinearAuthContext,
        first: usize,
    ) -> Result<Vec<LinearProject>, String> {
        let limit = first.max(1);
        let page_size = limit.clamp(1, LINEAR_CONNECTION_PAGE_SIZE);
        let mut after: Option<String> = None;
        let mut projects = Vec::new();
        while projects.len() < limit {
            let data = self
                .graphql(
                    &auth.api_token,
                    linear_projects_query(),
                    serde_json::json!({
                        "first": page_size as i64,
                        "after": after,
                    }),
                )
                .await?;
            let page = projects_from_data(&data)?;
            let count = page.len();
            projects.extend(page);
            if projects.len() >= limit {
                projects.truncate(limit);
                break;
            }
            if count == 0 {
                break;
            }
            let page_info = page_info_from_connection(&data, "projects");
            if !page_info.has_next_page {
                break;
            }
            after = page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }
        Ok(projects)
    }

    async fn list_issue_team_labels(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
    ) -> Result<Vec<LinearLabel>, String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let mut after: Option<String> = None;
        let mut labels = Vec::new();
        loop {
            let data = self
                .graphql(
                    &auth.api_token,
                    linear_issue_team_labels_query(),
                    serde_json::json!({
                        "id": issue_id,
                        "first": LINEAR_LABEL_PAGE_SIZE as i64,
                        "after": after,
                    }),
                )
                .await?;
            let page = issue_team_labels_from_data(&data)?;
            let count = page.len();
            labels.extend(page);
            if count == 0 {
                break;
            }
            let page_info = issue_team_labels_page_info_from_data(&data);
            if !page_info.has_next_page {
                break;
            }
            after = page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }
        Ok(labels)
    }

    async fn update_issue_labels(
        &self,
        auth: &LinearAuthContext,
        issue_id: &str,
        label_ids: Vec<String>,
    ) -> Result<(), String> {
        let issue_id = required_trimmed(issue_id, "Linear issue id is required")?;
        let data = self
            .graphql(
                &auth.api_token,
                linear_issue_update_mutation(),
                serde_json::json!({
                    "id": issue_id,
                    "input": { "labelIds": label_ids },
                }),
            )
            .await?;
        mutation_success(&data, "issueUpdate")
    }
}

fn linear_issue_team_labels_query() -> &'static str {
    r#"
    query RalphXLinearIssueTeamLabels($id: String!, $first: Int!, $after: String) {
      issue(id: $id) {
        team {
          id
          labels(first: $first, after: $after) {
            nodes {
              id
              name
            }
            pageInfo {
              hasNextPage
              endCursor
            }
          }
        }
      }
    }
    "#
}

fn issue_team_labels_from_data(data: &Value) -> Result<Vec<LinearLabel>, String> {
    let team = data
        .get("issue")
        .and_then(|issue| issue.get("team"))
        .filter(|team| !team.is_null())
        .ok_or_else(|| "Could not resolve Linear team for this issue".to_string())?;
    let nodes = team
        .get("labels")
        .and_then(|labels| labels.get("nodes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(nodes.iter().filter_map(label_from_node).collect())
}

fn issue_team_labels_page_info_from_data(data: &Value) -> LinearPageInfo {
    data.get("issue")
        .and_then(|issue| issue.get("team"))
        .and_then(|team| team.get("labels"))
        .map(page_info_from_value)
        .unwrap_or_default()
}

fn label_from_node(node: &Value) -> Option<LinearLabel> {
    let id = node
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let name = node
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(LinearLabel {
        id: id.to_string(),
        name: name.to_string(),
    })
}

fn linear_projects_query() -> &'static str {
    r#"
    query RalphXLinearProjects($first: Int!, $after: String) {
      projects(first: $first, after: $after) {
        nodes {
          id
          name
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
    "#
}

fn projects_from_data(data: &Value) -> Result<Vec<LinearProject>, String> {
    let nodes = data
        .get("projects")
        .and_then(|projects| projects.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .ok_or_else(|| "Linear projects response was malformed".to_string())?;
    Ok(nodes.iter().filter_map(project_from_node).collect())
}

fn project_from_node(node: &Value) -> Option<LinearProject> {
    let id = node.get("id").and_then(|id| id.as_str())?.trim();
    if id.is_empty() {
        return None;
    }
    let name = node
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(id);
    Some(LinearProject {
        id: id.to_string(),
        name: name.to_string(),
    })
}

fn linear_issue_search_query() -> &'static str {
    r#"
    query RalphXLinearIssueSearch($term: String!, $first: Int!, $after: String) {
      searchIssues(term: $term, first: $first, after: $after) {
        nodes {
          id
          identifier
          title
          url
          description
          state {
            id
            name
            type
            color
          }
          assignee {
            name
          }
          labels {
            nodes {
              name
            }
          }
          project {
            name
          }
          updatedAt
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
    "#
}

fn linear_recent_issues_query() -> &'static str {
    r#"
    query RalphXLinearRecentIssues($first: Int!, $after: String) {
      issues(first: $first, after: $after, orderBy: updatedAt) {
        nodes {
          id
          identifier
          title
          url
          description
          state {
            id
            name
            type
            color
          }
          assignee {
            name
          }
          labels {
            nodes {
              name
            }
          }
          project {
            name
          }
          updatedAt
        }
        pageInfo {
          hasNextPage
          endCursor
        }
      }
    }
    "#
}

fn linear_workflow_states_query(
    team_id: Option<&str>,
    after: Option<&str>,
) -> (&'static str, Value) {
    match team_id.map(str::trim).filter(|value| !value.is_empty()) {
        Some(team_id) => (
            r#"
            query RalphXLinearWorkflowStates($teamId: String!, $after: String) {
              workflowStates(filter: { team: { id: { eq: $teamId } } }, first: 100, after: $after) {
                nodes {
                  id
                  name
                  type
                  color
                  position
                }
                pageInfo {
                  hasNextPage
                  endCursor
                }
              }
            }
            "#,
            serde_json::json!({ "teamId": team_id, "after": after }),
        ),
        None => (
            r#"
            query RalphXLinearWorkflowStates($after: String) {
              workflowStates(first: 100, after: $after) {
                nodes {
                  id
                  name
                  type
                  color
                  position
                }
                pageInfo {
                  hasNextPage
                  endCursor
                }
              }
            }
            "#,
            serde_json::json!({ "after": after }),
        ),
    }
}

fn linear_issue_update_mutation() -> &'static str {
    r#"
    mutation RalphXLinearIssueUpdate($id: String!, $input: IssueUpdateInput!) {
      issueUpdate(id: $id, input: $input) {
        success
      }
    }
    "#
}

fn linear_comment_create_mutation() -> &'static str {
    r#"
    mutation RalphXLinearCommentCreate($input: CommentCreateInput!) {
      commentCreate(input: $input) {
        success
        comment {
          id
          body
          createdAt
          updatedAt
          user {
            id
            name
          }
        }
      }
    }
    "#
}

fn render_http_error(status: u16, bytes: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if let Some(errors) = value.get("errors").and_then(|value| value.as_array()) {
            if !errors.is_empty() {
                return format!(
                    "Linear returned HTTP {status}: {}",
                    render_graphql_errors(errors)
                );
            }
        }
    }
    let body = String::from_utf8_lossy(bytes).trim().to_string();
    if body.is_empty() {
        format!("Linear returned HTTP {status}")
    } else {
        format!("Linear returned HTTP {status}: {body}")
    }
}

fn validate_viewer_data(data: &Value) -> Result<(), String> {
    if data
        .get("viewer")
        .and_then(|viewer| viewer.get("id"))
        .is_some()
    {
        Ok(())
    } else {
        Err("Linear credentials did not return a viewer".to_string())
    }
}

fn current_user_from_data(data: &Value) -> Result<LinearUser, String> {
    let viewer = data
        .get("viewer")
        .ok_or_else(|| "Linear current user response did not include viewer".to_string())?;
    let id = viewer
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Linear current user response did not include viewer id".to_string())?;
    Ok(LinearUser {
        id: id.to_string(),
        name: viewer
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn workflow_states_from_data(data: &Value) -> Result<Vec<LinearWorkflowState>, String> {
    let nodes = data
        .get("workflowStates")
        .and_then(|states| states.get("nodes"))
        .and_then(Value::as_array)
        .ok_or_else(|| "Linear workflow states response did not include states".to_string())?;
    // Order to match Linear's view (Triage, In Progress, Todo, Backlog, Done,
    // Canceled), then by the state's position within its type.
    let mut ordered: Vec<(i32, f64, LinearWorkflowState)> = nodes
        .iter()
        .filter_map(|node| {
            let state = workflow_state_from_node(node)?;
            let state_type = node.get("type").and_then(Value::as_str).unwrap_or(&state.name);
            let position = node
                .get("position")
                .and_then(Value::as_f64)
                .unwrap_or(f64::MAX);
            Some((linear_state_sort_priority(state_type), position, state))
        })
        .collect();
    ordered.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then_with(|| left.1.partial_cmp(&right.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    Ok(ordered.into_iter().map(|(_, _, state)| state).collect())
}

/// Linear view ordering priority by workflow-state type. "unstarted" is checked
/// before "started" because the former contains the latter as a substring.
fn linear_state_sort_priority(value: &str) -> i32 {
    let value = value.to_ascii_lowercase();
    if value.contains("triage") {
        0
    } else if value.contains("unstarted") {
        2
    } else if value.contains("started") || value.contains("progress") || value.contains("review") {
        1
    } else if value.contains("todo") || value.contains("to do") {
        2
    } else if value.contains("backlog") {
        3
    } else if value.contains("completed")
        || value.contains("done")
        || value.contains("closed")
        || value.contains("resolved")
    {
        4
    } else if value.contains("canceled") || value.contains("cancelled") {
        5
    } else {
        6
    }
}

fn workflow_state_from_node(node: &Value) -> Option<LinearWorkflowState> {
    let id = node.get("id")?.as_str()?.trim();
    let name = node.get("name")?.as_str()?.trim();
    if id.is_empty() || name.is_empty() {
        return None;
    }
    let state_type = node.get("type").and_then(Value::as_str).unwrap_or(name);
    Some(LinearWorkflowState {
        id: id.to_string(),
        name: name.to_string(),
        category: linear_state_category(state_type),
        color: node
            .get("color")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

fn linear_state_category(value: &str) -> String {
    let value = value.to_ascii_lowercase();
    if value.contains("completed")
        || value.contains("done")
        || value.contains("closed")
        || value.contains("resolved")
    {
        "done".to_string()
    } else if value.contains("unstarted")
        || value.contains("backlog")
        || value.contains("todo")
        || value.contains("to do")
    {
        "todo".to_string()
    } else if value.contains("started")
        || value.contains("progress")
        || value.contains("review")
        || value.contains("active")
    {
        "in_progress".to_string()
    } else {
        "other".to_string()
    }
}

fn comment_from_create_data(data: &Value) -> Result<LinearComment, String> {
    mutation_success(data, "commentCreate")?;
    let comment = data
        .get("commentCreate")
        .and_then(|create| create.get("comment"))
        .ok_or_else(|| "Linear comment response did not include comment".to_string())?;
    let id = comment
        .get("id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Linear comment response did not include comment id".to_string())?;
    Ok(LinearComment {
        id: id.to_string(),
        body: comment
            .get("body")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        author_id: comment
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string),
        author_name: comment
            .get("user")
            .and_then(|user| user.get("name"))
            .and_then(Value::as_str)
            .map(str::to_string),
        created_at: comment
            .get("createdAt")
            .and_then(Value::as_str)
            .map(str::to_string),
        updated_at: comment
            .get("updatedAt")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn mutation_success(data: &Value, field: &str) -> Result<(), String> {
    let success = data
        .get(field)
        .and_then(|value| value.get("success"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if success {
        Ok(())
    } else {
        Err(format!("Linear {field} mutation did not succeed"))
    }
}

fn required_trimmed<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    let value = value.trim();
    if value.is_empty() {
        Err(message.to_string())
    } else {
        Ok(value)
    }
}

#[derive(Default)]
struct LinearPageInfo {
    has_next_page: bool,
    end_cursor: Option<String>,
}

fn page_info_from_connection(data: &Value, connection_key: &str) -> LinearPageInfo {
    data.get(connection_key)
        .map(page_info_from_value)
        .unwrap_or_default()
}

fn page_info_from_value(connection: &Value) -> LinearPageInfo {
    let page_info = connection.get("pageInfo").unwrap_or(&Value::Null);
    LinearPageInfo {
        has_next_page: page_info
            .get("hasNextPage")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        end_cursor: page_info
            .get("endCursor")
            .and_then(Value::as_str)
            .map(str::to_string),
    }
}

fn search_issue_summaries_from_data(data: &Value) -> Result<Vec<LinearIssueSummary>, String> {
    let nodes = data
        .get("searchIssues")
        .or_else(|| data.get("issues"))
        .and_then(|issues| issues.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .ok_or_else(|| "Linear search response did not include issues".to_string())?;
    Ok(nodes
        .iter()
        .filter_map(issue_summary_from_node)
        .collect::<Vec<_>>())
}

fn issue_content_from_data(data: &Value, reference_id: &str) -> Result<LinearIssueContent, String> {
    let issue = data
        .get("issue")
        .ok_or_else(|| "Linear issue response did not include issue".to_string())?;
    issue_content_from_node(issue).ok_or_else(|| {
        format!("Linear issue response did not include readable issue {reference_id}")
    })
}

fn issue_summary_from_node(node: &Value) -> Option<LinearIssueSummary> {
    let state = node.get("state");
    let state_type = state.and_then(|state| state.get("type")).and_then(Value::as_str);
    let state_name = state
        .and_then(|state| state.get("name"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let state_category = state_type
        .or(state_name.as_deref())
        .map(linear_state_category);
    Some(LinearIssueSummary {
        id: node.get("id")?.as_str()?.to_string(),
        key: node
            .get("identifier")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        title: node.get("title")?.as_str()?.to_string(),
        url: node
            .get("url")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        excerpt: node
            .get("description")
            .and_then(|value| value.as_str())
            .map(trim_excerpt),
        state_id: state
            .and_then(|state| state.get("id"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        state_name,
        state_category,
        state_color: state
            .and_then(|state| state.get("color"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        assignee: linear_user_name(node.get("assignee")),
        updated_at: node
            .get("updatedAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        labels: linear_label_names(node.get("labels")),
        project: linear_project_name(node.get("project")),
    })
}

fn issue_content_from_node(node: &Value) -> Option<LinearIssueContent> {
    Some(LinearIssueContent {
        id: node.get("id")?.as_str()?.to_string(),
        key: node
            .get("identifier")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        title: node.get("title")?.as_str()?.to_string(),
        url: node
            .get("url")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        body: node
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string(),
        state_name: node
            .get("state")
            .and_then(|state| state.get("name"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        assignee: linear_user_name(node.get("assignee")),
        creator: linear_user_name(node.get("creator")),
        updated_at: node
            .get("updatedAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        comments: issue_comments_from_node(node),
        attachments: issue_attachments_from_node(node),
        labels: linear_label_names(node.get("labels")),
        project: linear_project_name(node.get("project")),
    })
}

fn linear_label_names(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(|labels| labels.get("nodes"))
        .and_then(Value::as_array)
        .map(|nodes| {
            nodes
                .iter()
                .filter_map(|node| node.get("name").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn linear_project_name(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|project| project.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn issue_comments_from_node(node: &Value) -> Vec<LinearComment> {
    node.get("comments")
        .and_then(|comments| comments.get("nodes"))
        .and_then(|nodes| nodes.as_array())
        .map(|nodes| nodes.iter().filter_map(linear_comment_from_node).collect())
        .unwrap_or_default()
}

fn issue_attachments_from_node(node: &Value) -> Vec<LinearAttachment> {
    node.get("attachments")
        .and_then(|attachments| attachments.get("nodes"))
        .and_then(Value::as_array)
        .map(|nodes| nodes.iter().filter_map(linear_attachment_from_node).collect())
        .unwrap_or_default()
}

fn linear_attachment_from_node(node: &Value) -> Option<LinearAttachment> {
    let id = node.get("id")?.as_str()?.to_string();
    let url = node.get("url")?.as_str()?.trim().to_string();
    if url.is_empty() {
        return None;
    }
    let title = node
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&url)
        .to_string();
    Some(LinearAttachment {
        id,
        title,
        subtitle: node
            .get("subtitle")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        url,
    })
}

fn linear_comment_from_node(node: &Value) -> Option<LinearComment> {
    Some(LinearComment {
        id: node.get("id")?.as_str()?.to_string(),
        body: node.get("body")?.as_str()?.to_string(),
        author_id: node
            .get("user")
            .and_then(|user| user.get("id"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
        author_name: linear_user_name(node.get("user")),
        created_at: node
            .get("createdAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        updated_at: node
            .get("updatedAt")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    })
}

fn linear_user_name(value: Option<&Value>) -> Option<String> {
    value
        .and_then(|user| user.get("name"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn trim_excerpt(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() <= 240 {
        return trimmed.to_string();
    }
    let mut end = 240;
    while !trimmed.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &trimmed[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_search_query_uses_current_linear_search_field() {
        let query = linear_issue_search_query();

        assert!(query.contains("searchIssues(term: $term, first: $first, after: $after)"));
        assert!(query.contains("pageInfo"));
        assert!(!query.contains("issues(search:"));
        assert!(!query.contains("issueSearch"));
    }

    #[test]
    fn recent_issues_query_orders_by_updated_at() {
        let query = linear_recent_issues_query();

        assert!(query.contains("issues(first: $first, after: $after, orderBy: updatedAt)"));
        assert!(query.contains("pageInfo"));
        assert!(!query.contains("searchIssues"));
        assert!(!query.contains("term:"));
    }

    #[test]
    fn projects_query_uses_projects_connection() {
        let query = linear_projects_query();

        assert!(query.contains("projects(first: $first, after: $after)"));
        assert!(query.contains("nodes"));
        assert!(query.contains("pageInfo"));
    }

    #[test]
    fn page_info_from_connection_reads_cursor_state() {
        let data = serde_json::json!({
            "projects": {
                "pageInfo": {
                    "hasNextPage": true,
                    "endCursor": "cursor-2"
                }
            }
        });

        let page_info = page_info_from_connection(&data, "projects");

        assert!(page_info.has_next_page);
        assert_eq!(page_info.end_cursor.as_deref(), Some("cursor-2"));
    }

    #[test]
    fn page_info_from_connection_defaults_when_missing() {
        let page_info = page_info_from_connection(&serde_json::json!({}), "projects");

        assert!(!page_info.has_next_page);
        assert!(page_info.end_cursor.is_none());
    }

    #[test]
    fn issue_team_labels_page_info_reads_nested_labels_connection() {
        let data = serde_json::json!({
            "issue": {
                "team": {
                    "labels": {
                        "pageInfo": {
                            "hasNextPage": true,
                            "endCursor": "label-cursor"
                        }
                    }
                }
            }
        });

        let page_info = issue_team_labels_page_info_from_data(&data);

        assert!(page_info.has_next_page);
        assert_eq!(page_info.end_cursor.as_deref(), Some("label-cursor"));
    }

    #[test]
    fn projects_from_data_parses_ids_and_names() {
        let data = serde_json::json!({
            "projects": {
                "nodes": [
                    { "id": "p1", "name": "FLUX PT" },
                    { "id": "p2", "name": "  " },
                    { "id": "", "name": "skip-me" },
                ]
            }
        });

        let projects = projects_from_data(&data).expect("projects parse");

        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, "p1");
        assert_eq!(projects[0].name, "FLUX PT");
        // Blank name falls back to the id; the empty-id node is skipped.
        assert_eq!(projects[1].id, "p2");
        assert_eq!(projects[1].name, "p2");
    }

    #[test]
    fn projects_from_data_rejects_malformed_payload() {
        let data = serde_json::json!({ "projects": {} });
        assert!(projects_from_data(&data).is_err());
    }

    #[test]
    fn http_error_includes_linear_graphql_message() {
        let body = br#"{
            "errors": [
                { "message": "Unknown argument \"search\" on field \"Query.issues\"." }
            ]
        }"#;

        let rendered = render_http_error(400, body);

        assert_eq!(
            rendered,
            "Linear returned HTTP 400: Unknown argument \"search\" on field \"Query.issues\"."
        );
    }

    #[test]
    fn http_error_falls_back_to_status_or_body_text() {
        assert_eq!(render_http_error(401, b""), "Linear returned HTTP 401");
        assert_eq!(
            render_http_error(503, b"temporarily unavailable\n"),
            "Linear returned HTTP 503: temporarily unavailable"
        );
    }

    #[test]
    fn viewer_validation_requires_viewer_id() {
        assert!(validate_viewer_data(&serde_json::json!({
            "viewer": { "id": "viewer-id", "name": "User" }
        }))
        .is_ok());

        assert_eq!(
            validate_viewer_data(&serde_json::json!({ "viewer": { "name": "User" } })).unwrap_err(),
            "Linear credentials did not return a viewer"
        );
    }

    #[test]
    fn current_user_from_data_requires_viewer_id() {
        let user = current_user_from_data(&serde_json::json!({
            "viewer": { "id": "viewer-id", "name": "A. User" }
        }))
        .expect("viewer should parse");

        assert_eq!(user.id, "viewer-id");
        assert_eq!(user.name.as_deref(), Some("A. User"));

        assert_eq!(
            current_user_from_data(&serde_json::json!({ "viewer": { "name": "A. User" } }))
                .unwrap_err(),
            "Linear current user response did not include viewer id"
        );
    }

    #[test]
    fn workflow_states_from_data_maps_states_to_ticket_categories() {
        let states = workflow_states_from_data(&serde_json::json!({
            "workflowStates": {
                "nodes": [
                    { "id": "state-1", "name": "Todo", "type": "unstarted", "color": "#bec2c8" },
                    { "id": "state-2", "name": "Doing", "type": "started" },
                    { "id": "state-3", "name": "Done", "type": "completed" }
                ]
            }
        }))
        .expect("states should parse");

        assert_eq!(states.len(), 3);
        // Sorted to match Linear's view order: In Progress, Todo, Done.
        assert_eq!(states[0].category, "in_progress");
        assert_eq!(states[1].category, "todo");
        assert_eq!(states[2].category, "done");
        assert_eq!(states[1].color.as_deref(), Some("#bec2c8"));
    }

    #[test]
    fn workflow_states_ordered_like_linear_view() {
        let states = workflow_states_from_data(&serde_json::json!({
            "workflowStates": {
                "nodes": [
                    { "id": "done", "name": "Done", "type": "completed", "position": 4 },
                    { "id": "backlog", "name": "Backlog", "type": "backlog", "position": 0 },
                    { "id": "todo", "name": "Todo", "type": "unstarted", "position": 2 },
                    { "id": "doing", "name": "In Progress", "type": "started", "position": 3 }
                ]
            }
        }))
        .expect("states should parse");

        assert_eq!(
            states.iter().map(|state| state.id.as_str()).collect::<Vec<_>>(),
            vec!["doing", "todo", "backlog", "done"],
        );
    }

    #[test]
    fn comment_from_create_data_maps_created_comment() {
        let comment = comment_from_create_data(&serde_json::json!({
            "commentCreate": {
                "success": true,
                "comment": {
                    "id": "comment-1",
                    "body": "Ready",
                    "createdAt": "2026-06-20T08:00:00Z",
                    "updatedAt": "2026-06-20T08:00:00Z",
                    "user": { "id": "user-1", "name": "A. User" }
                }
            }
        }))
        .expect("comment should parse");

        assert_eq!(comment.id, "comment-1");
        assert_eq!(comment.body, "Ready");
        assert_eq!(comment.author_name.as_deref(), Some("A. User"));
    }

    #[test]
    fn search_issue_summaries_from_data_filters_unreadable_nodes() {
        let data = serde_json::json!({
            "searchIssues": {
                "nodes": [
                    {
                        "id": "issue-1",
                        "identifier": "LIN-1",
                        "title": "Readable",
                        "description": " first issue ",
                        "state": { "id": "state-todo", "name": "Todo", "type": "unstarted", "color": "#bec2c8" },
                        "labels": { "nodes": [{ "name": "backend" }] },
                        "project": { "name": "Platform" }
                    },
                    {
                        "id": "issue-2"
                    }
                ]
            }
        });

        let issues = search_issue_summaries_from_data(&data).expect("search data should parse");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].id, "issue-1");
        assert_eq!(issues[0].key.as_deref(), Some("LIN-1"));
        assert_eq!(issues[0].title, "Readable");
        assert_eq!(issues[0].excerpt.as_deref(), Some("first issue"));
        assert_eq!(issues[0].state_id.as_deref(), Some("state-todo"));
        assert_eq!(issues[0].state_name.as_deref(), Some("Todo"));
        assert_eq!(issues[0].state_category.as_deref(), Some("todo"));
        assert_eq!(issues[0].state_color.as_deref(), Some("#bec2c8"));
        assert_eq!(issues[0].labels, vec!["backend".to_string()]);
        assert_eq!(issues[0].project.as_deref(), Some("Platform"));
    }

    #[test]
    fn search_issue_summaries_from_data_accepts_recent_issues_connection() {
        let data = serde_json::json!({
            "issues": {
                "nodes": [
                    {
                        "id": "issue-1",
                        "identifier": "LIN-1",
                        "title": "Recent",
                        "description": " latest issue ",
                        "state": { "name": "In Progress" }
                    }
                ]
            }
        });

        let issues = search_issue_summaries_from_data(&data).expect("recent issues should parse");

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key.as_deref(), Some("LIN-1"));
        assert_eq!(issues[0].title, "Recent");
        assert_eq!(issues[0].state_name.as_deref(), Some("In Progress"));
    }

    #[test]
    fn search_issue_summaries_from_data_maps_multiple_optional_shapes() {
        let data = serde_json::json!({
            "searchIssues": {
                "nodes": [
                    {
                        "id": "issue-1",
                        "identifier": "LIN-1",
                        "title": "With all optional fields",
                        "url": "https://linear.app/acme/issue/LIN-1/all",
                        "description": "All fields",
                        "state": { "name": "Todo" },
                        "assignee": { "name": "A. User" },
                        "updatedAt": "2026-06-21T08:00:00Z"
                    },
                    {
                        "id": "issue-2",
                        "identifier": null,
                        "title": "Without optional strings",
                        "url": null,
                        "description": null,
                        "state": null
                    },
                    {
                        "id": "issue-3",
                        "identifier": "LIN-3",
                        "title": "Without state name",
                        "url": "https://linear.app/acme/issue/LIN-3/no-state-name",
                        "description": "No state name",
                        "state": {}
                    }
                ]
            }
        });

        let issues = search_issue_summaries_from_data(&data).expect("search data should parse");

        assert_eq!(issues.len(), 3);
        assert_eq!(issues[0].id, "issue-1");
        assert_eq!(issues[0].key.as_deref(), Some("LIN-1"));
        assert_eq!(issues[0].title, "With all optional fields");
        assert_eq!(
            issues[0].url.as_deref(),
            Some("https://linear.app/acme/issue/LIN-1/all")
        );
        assert_eq!(issues[0].excerpt.as_deref(), Some("All fields"));
        assert_eq!(issues[0].state_name.as_deref(), Some("Todo"));
        assert_eq!(issues[0].assignee.as_deref(), Some("A. User"));
        assert_eq!(issues[0].updated_at.as_deref(), Some("2026-06-21T08:00:00Z"));

        assert_eq!(issues[1].id, "issue-2");
        assert!(issues[1].key.is_none());
        assert_eq!(issues[1].title, "Without optional strings");
        assert!(issues[1].url.is_none());
        assert!(issues[1].excerpt.is_none());
        assert!(issues[1].state_name.is_none());
        assert!(issues[1].assignee.is_none());
        assert!(issues[1].updated_at.is_none());

        assert_eq!(issues[2].id, "issue-3");
        assert_eq!(issues[2].key.as_deref(), Some("LIN-3"));
        assert_eq!(issues[2].title, "Without state name");
        assert_eq!(
            issues[2].url.as_deref(),
            Some("https://linear.app/acme/issue/LIN-3/no-state-name")
        );
        assert_eq!(issues[2].excerpt.as_deref(), Some("No state name"));
        assert!(issues[2].state_name.is_none());
    }

    #[test]
    fn search_issue_summaries_from_data_requires_nodes_array() {
        let error = search_issue_summaries_from_data(&serde_json::json!({
            "searchIssues": {}
        }))
        .unwrap_err();

        assert_eq!(error, "Linear search response did not include issues");
    }

    #[test]
    fn issue_content_from_data_maps_issue_and_reports_missing_issue() {
        let content = issue_content_from_data(
            &serde_json::json!({
            "issue": {
                "id": "issue-1",
                "identifier": "LIN-1",
                "title": "Readable",
                "url": "https://linear.app/acme/issue/LIN-1/readable",
                "description": "Issue body",
                "state": { "name": "In Progress" },
                "assignee": { "name": "A. User" },
                "creator": { "name": "C. User" },
                "labels": { "nodes": [{ "name": "backend" }, { "name": "urgent" }] },
                "project": { "name": "Platform" },
                "updatedAt": "2026-06-18T08:00:00Z",
                "attachments": {
                    "nodes": [
                        {
                            "id": "attachment-1",
                            "title": "Design mock",
                            "subtitle": "Figma",
                            "url": "https://uploads.linear.app/design.png"
                        }
                    ]
                },
                "comments": {
                    "nodes": [
                        {
                            "id": "comment-1",
                            "body": "Looks good",
                            "createdAt": "2026-06-18T09:00:00Z",
                            "updatedAt": "2026-06-18T09:01:00Z",
                            "user": { "id": "user-1", "name": "Reviewer" }
                        }
                    ]
                }
            }
            }),
            "issue-1",
        )
        .expect("issue data should parse");

        assert_eq!(content.id, "issue-1");
        assert_eq!(content.key.as_deref(), Some("LIN-1"));
        assert_eq!(content.title, "Readable");
        assert_eq!(content.body, "Issue body");
        assert_eq!(content.state_name.as_deref(), Some("In Progress"));
        assert_eq!(content.assignee.as_deref(), Some("A. User"));
        assert_eq!(content.creator.as_deref(), Some("C. User"));
        assert_eq!(content.labels, vec!["backend".to_string(), "urgent".to_string()]);
        assert_eq!(content.project.as_deref(), Some("Platform"));
        assert_eq!(content.updated_at.as_deref(), Some("2026-06-18T08:00:00Z"));
        assert_eq!(content.attachments.len(), 1);
        assert_eq!(content.attachments[0].title, "Design mock");
        assert_eq!(
            content.attachments[0].url.as_str(),
            "https://uploads.linear.app/design.png"
        );
        assert_eq!(content.comments.len(), 1);
        assert_eq!(content.comments[0].id, "comment-1");
        assert_eq!(content.comments[0].body, "Looks good");
        assert_eq!(content.comments[0].author_name.as_deref(), Some("Reviewer"));

        assert_eq!(
            issue_content_from_data(&serde_json::json!({}), "missing").unwrap_err(),
            "Linear issue response did not include issue"
        );
        assert_eq!(
            issue_content_from_data(
                &serde_json::json!({ "issue": { "id": "issue-1" } }),
                "issue-1",
            )
            .unwrap_err(),
            "Linear issue response did not include readable issue issue-1"
        );
    }

    #[test]
    fn http_error_with_unreadable_graphql_errors_falls_back_to_body() {
        let body = br#"{"errors":[{"extensions":{"code":"bad"}}]}"#;

        assert_eq!(render_http_error(500, body), "Linear returned HTTP 500: ");
        assert_eq!(render_graphql_errors(&[]), "");
    }

    #[test]
    fn issue_summary_from_node_keeps_absent_optional_fields_empty() {
        let node = serde_json::json!({
            "id": "issue-id",
            "title": "Required fields only"
        });

        let summary = issue_summary_from_node(&node).expect("required fields should parse");

        assert_eq!(summary.id, "issue-id");
        assert_eq!(summary.title, "Required fields only");
        assert!(summary.key.is_none());
        assert!(summary.url.is_none());
        assert!(summary.excerpt.is_none());
        assert!(summary.state_name.is_none());
    }

    #[test]
    fn trim_excerpt_preserves_short_text_and_truncates_on_char_boundary() {
        assert_eq!(trim_excerpt("  short text  "), "short text");

        let long = format!("{}{}", "a".repeat(239), "ééé");
        let excerpt = trim_excerpt(&long);

        assert!(excerpt.ends_with("..."));
        assert!(excerpt.len() <= 243);
        assert!(excerpt.is_char_boundary(excerpt.len() - 3));
    }

    #[test]
    fn issue_summary_from_node_maps_optional_fields_and_trims_excerpt() {
        let long_description = format!("{}tail", "a".repeat(240));
        let node = serde_json::json!({
            "id": "issue-id",
            "identifier": "LIN-123",
            "title": "Example",
            "url": "https://linear.app/acme/issue/LIN-123/example",
            "description": long_description,
            "state": { "name": "In Progress" },
            "assignee": { "name": "A. User" },
            "updatedAt": "2026-06-21T08:00:00Z"
        });

        let summary = issue_summary_from_node(&node).expect("node should parse");

        assert_eq!(summary.id, "issue-id");
        assert_eq!(summary.key.as_deref(), Some("LIN-123"));
        assert_eq!(summary.title, "Example");
        assert_eq!(
            summary.url.as_deref(),
            Some("https://linear.app/acme/issue/LIN-123/example")
        );
        assert_eq!(summary.excerpt.as_deref().unwrap().len(), 243);
        assert!(summary.excerpt.as_deref().unwrap().ends_with("..."));
        assert_eq!(summary.state_name.as_deref(), Some("In Progress"));
        assert_eq!(summary.assignee.as_deref(), Some("A. User"));
        assert_eq!(summary.updated_at.as_deref(), Some("2026-06-21T08:00:00Z"));
    }

    #[test]
    fn issue_summary_from_node_rejects_unreadable_required_fields() {
        let missing_id = serde_json::json!({
            "title": "Example"
        });

        assert!(issue_summary_from_node(&missing_id).is_none());
    }

    #[test]
    fn issue_content_from_node_maps_missing_optional_fields_to_empty_body() {
        let node = serde_json::json!({
            "id": "issue-id",
            "title": "Example"
        });

        let content = issue_content_from_node(&node).expect("node should parse");

        assert_eq!(content.id, "issue-id");
        assert_eq!(content.title, "Example");
        assert!(content.key.is_none());
        assert!(content.url.is_none());
        assert!(content.body.is_empty());
        assert!(content.state_name.is_none());
        assert!(content.assignee.is_none());
        assert!(content.creator.is_none());
        assert!(content.updated_at.is_none());
        assert!(content.comments.is_empty());
        assert!(content.labels.is_empty());
        assert!(content.project.is_none());
    }

    #[test]
    fn issue_content_from_node_maps_optional_fields() {
        let node = serde_json::json!({
            "id": "issue-id",
            "identifier": "LIN-456",
            "title": "Fetched issue",
            "url": "https://linear.app/acme/issue/LIN-456/fetched",
            "description": "Fetched issue body",
            "state": { "name": "Done" },
            "assignee": { "name": "A. User" },
            "creator": { "name": "C. User" },
            "labels": { "nodes": [{ "name": "frontend" }] },
            "project": { "name": "Experience" },
            "updatedAt": "2026-06-18T08:15:00Z",
            "comments": {
                "nodes": [
                    {
                        "id": "comment-2",
                        "body": "Provider comment",
                        "user": { "id": "user-2", "name": "Author" }
                    }
                ]
            }
        });

        let content = issue_content_from_node(&node).expect("node should parse");

        assert_eq!(content.id, "issue-id");
        assert_eq!(content.key.as_deref(), Some("LIN-456"));
        assert_eq!(content.title, "Fetched issue");
        assert_eq!(
            content.url.as_deref(),
            Some("https://linear.app/acme/issue/LIN-456/fetched")
        );
        assert_eq!(content.body, "Fetched issue body");
        assert_eq!(content.state_name.as_deref(), Some("Done"));
        assert_eq!(content.assignee.as_deref(), Some("A. User"));
        assert_eq!(content.creator.as_deref(), Some("C. User"));
        assert_eq!(content.labels, vec!["frontend".to_string()]);
        assert_eq!(content.project.as_deref(), Some("Experience"));
        assert_eq!(content.updated_at.as_deref(), Some("2026-06-18T08:15:00Z"));
        assert_eq!(content.comments.len(), 1);
        assert_eq!(content.comments[0].author_id.as_deref(), Some("user-2"));
        assert_eq!(content.comments[0].author_name.as_deref(), Some("Author"));
    }

    #[test]
    fn graphql_error_rendering_ignores_entries_without_message() {
        let errors = vec![
            serde_json::json!({ "message": "first" }),
            serde_json::json!({ "extensions": { "code": "bad" } }),
            serde_json::json!({ "message": "second" }),
        ];

        assert_eq!(render_graphql_errors(&errors), "first; second");
    }

    #[test]
    fn issue_team_labels_query_scopes_to_issue_team() {
        let query = linear_issue_team_labels_query();
        assert!(query.contains("issue(id: $id)"));
        assert!(query.contains("team {"));
        assert!(query.contains("labels(first: $first, after: $after)"));
        assert!(query.contains("pageInfo"));
    }

    #[test]
    fn issue_team_labels_from_data_parses_id_and_name_nodes() {
        let labels = issue_team_labels_from_data(&serde_json::json!({
            "issue": {
                "team": {
                    "id": "team-1",
                    "labels": {
                        "nodes": [
                            { "id": "label-1", "name": "Bug" },
                            { "id": "label-2", "name": "  " },
                            { "id": "label-3", "name": "Feature" }
                        ]
                    }
                }
            }
        }))
        .expect("labels should parse");

        // The blank-named node is dropped during parsing.
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].id, "label-1");
        assert_eq!(labels[0].name, "Bug");
        assert_eq!(labels[1].id, "label-3");
        assert_eq!(labels[1].name, "Feature");
    }

    #[test]
    fn issue_team_labels_from_data_errors_when_team_missing() {
        let error = issue_team_labels_from_data(&serde_json::json!({
            "issue": { "team": null }
        }))
        .expect_err("missing team should error");
        assert!(error.contains("Could not resolve Linear team"));
    }

    #[test]
    fn linear_state_sort_priority_covers_every_branch() {
        // "unstarted" must be checked before "started" (substring overlap).
        assert_eq!(linear_state_sort_priority("triage"), 0);
        assert_eq!(linear_state_sort_priority("started"), 1);
        assert_eq!(linear_state_sort_priority("In Progress"), 1);
        assert_eq!(linear_state_sort_priority("In Review"), 1);
        assert_eq!(linear_state_sort_priority("unstarted"), 2);
        assert_eq!(linear_state_sort_priority("Todo"), 2);
        assert_eq!(linear_state_sort_priority("to do"), 2);
        assert_eq!(linear_state_sort_priority("backlog"), 3);
        assert_eq!(linear_state_sort_priority("completed"), 4);
        assert_eq!(linear_state_sort_priority("done"), 4);
        assert_eq!(linear_state_sort_priority("closed"), 4);
        assert_eq!(linear_state_sort_priority("resolved"), 4);
        assert_eq!(linear_state_sort_priority("canceled"), 5);
        assert_eq!(linear_state_sort_priority("cancelled"), 5);
        assert_eq!(linear_state_sort_priority("something-else"), 6);
        assert_eq!(linear_state_sort_priority(""), 6);
    }

    #[test]
    fn linear_state_category_covers_every_branch() {
        assert_eq!(linear_state_category("completed"), "done");
        assert_eq!(linear_state_category("Done"), "done");
        assert_eq!(linear_state_category("closed"), "done");
        assert_eq!(linear_state_category("resolved"), "done");
        assert_eq!(linear_state_category("unstarted"), "todo");
        assert_eq!(linear_state_category("backlog"), "todo");
        assert_eq!(linear_state_category("Todo"), "todo");
        assert_eq!(linear_state_category("to do"), "todo");
        assert_eq!(linear_state_category("started"), "in_progress");
        assert_eq!(linear_state_category("In Progress"), "in_progress");
        assert_eq!(linear_state_category("review"), "in_progress");
        assert_eq!(linear_state_category("active"), "in_progress");
        assert_eq!(linear_state_category("mystery"), "other");
        assert_eq!(linear_state_category(""), "other");
    }

    #[test]
    fn workflow_state_from_node_parses_valid_node() {
        let state = workflow_state_from_node(&serde_json::json!({
            "id": " state-1 ",
            "name": " In Progress ",
            "type": "started",
            "color": " #f2c94c "
        }))
        .expect("valid node should parse");

        assert_eq!(state.id, "state-1");
        assert_eq!(state.name, "In Progress");
        assert_eq!(state.category, "in_progress");
        assert_eq!(state.color.as_deref(), Some("#f2c94c"));
    }

    #[test]
    fn workflow_state_from_node_falls_back_to_name_for_category() {
        // No "type" → category is derived from the name.
        let state = workflow_state_from_node(&serde_json::json!({
            "id": "state-1",
            "name": "Done"
        }))
        .expect("node without type should parse");

        assert_eq!(state.category, "done");
        assert!(state.color.is_none());
    }

    #[test]
    fn workflow_state_from_node_filters_out_empty_id_or_name() {
        assert!(workflow_state_from_node(&serde_json::json!({
            "id": "  ",
            "name": "Todo"
        }))
        .is_none());
        assert!(workflow_state_from_node(&serde_json::json!({
            "id": "state-1",
            "name": ""
        }))
        .is_none());
        assert!(workflow_state_from_node(&serde_json::json!({ "name": "Todo" })).is_none());
    }

    #[test]
    fn linear_user_name_extracts_trimmed_name() {
        assert_eq!(
            linear_user_name(Some(&serde_json::json!({ "name": "  A. User  " }))).as_deref(),
            Some("A. User")
        );
    }

    #[test]
    fn linear_user_name_returns_none_when_name_absent_or_blank() {
        assert!(linear_user_name(None).is_none());
        assert!(linear_user_name(Some(&serde_json::json!({ "id": "u1" }))).is_none());
        assert!(linear_user_name(Some(&serde_json::json!({ "name": "   " }))).is_none());
        assert!(linear_user_name(Some(&Value::Null)).is_none());
    }

    #[test]
    fn linear_label_names_filters_and_trims_label_nodes() {
        let labels = linear_label_names(Some(&serde_json::json!({
            "nodes": [
                { "name": " backend " },
                { "name": "" },
                { "name": "   " },
                { "id": "no-name" },
                { "name": "frontend" }
            ]
        })));

        assert_eq!(labels, vec!["backend".to_string(), "frontend".to_string()]);
    }

    #[test]
    fn linear_label_names_returns_empty_when_nodes_missing() {
        assert!(linear_label_names(None).is_empty());
        assert!(linear_label_names(Some(&serde_json::json!({}))).is_empty());
    }

    #[test]
    fn linear_project_name_extracts_and_trims() {
        assert_eq!(
            linear_project_name(Some(&serde_json::json!({ "name": "  Platform  " }))).as_deref(),
            Some("Platform")
        );
    }

    #[test]
    fn linear_project_name_returns_none_when_absent_or_blank() {
        assert!(linear_project_name(None).is_none());
        assert!(linear_project_name(Some(&serde_json::json!({ "name": "   " }))).is_none());
        assert!(linear_project_name(Some(&serde_json::json!({ "id": "p1" }))).is_none());
    }

    #[test]
    fn issue_comments_from_node_extracts_and_filters_comments() {
        let comments = issue_comments_from_node(&serde_json::json!({
            "comments": {
                "nodes": [
                    {
                        "id": "comment-1",
                        "body": "First",
                        "createdAt": "2026-06-20T08:00:00Z",
                        "user": { "id": "user-1", "name": "A. User" }
                    },
                    { "id": "comment-2" },
                    { "body": "no id" }
                ]
            }
        }));

        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, "comment-1");
        assert_eq!(comments[0].body, "First");
        assert_eq!(comments[0].author_id.as_deref(), Some("user-1"));
        assert_eq!(comments[0].author_name.as_deref(), Some("A. User"));
    }

    #[test]
    fn issue_comments_from_node_returns_empty_when_comments_missing() {
        assert!(issue_comments_from_node(&serde_json::json!({})).is_empty());
        assert!(issue_comments_from_node(&serde_json::json!({ "comments": {} })).is_empty());
    }

    #[test]
    fn linear_comment_from_node_parses_fields_and_optional_user() {
        let comment = linear_comment_from_node(&serde_json::json!({
            "id": "comment-1",
            "body": "Looks good",
            "createdAt": "2026-06-20T08:00:00Z",
            "updatedAt": "2026-06-20T09:00:00Z",
            "user": { "id": "user-1", "name": "A. User" }
        }))
        .expect("comment should parse");

        assert_eq!(comment.id, "comment-1");
        assert_eq!(comment.body, "Looks good");
        assert_eq!(comment.author_id.as_deref(), Some("user-1"));
        assert_eq!(comment.author_name.as_deref(), Some("A. User"));
        assert_eq!(comment.created_at.as_deref(), Some("2026-06-20T08:00:00Z"));
        assert_eq!(comment.updated_at.as_deref(), Some("2026-06-20T09:00:00Z"));
    }

    #[test]
    fn linear_comment_from_node_handles_missing_user() {
        let comment = linear_comment_from_node(&serde_json::json!({
            "id": "comment-1",
            "body": "No author"
        }))
        .expect("comment without user should parse");

        assert!(comment.author_id.is_none());
        assert!(comment.author_name.is_none());
        assert!(comment.created_at.is_none());
        assert!(comment.updated_at.is_none());
    }

    #[test]
    fn linear_comment_from_node_requires_id_and_body() {
        assert!(linear_comment_from_node(&serde_json::json!({ "body": "no id" })).is_none());
        assert!(linear_comment_from_node(&serde_json::json!({ "id": "comment-1" })).is_none());
    }

    #[test]
    fn issue_summary_from_node_maps_state_type_to_category() {
        let summary = issue_summary_from_node(&serde_json::json!({
            "id": "issue-1",
            "title": "Example",
            "state": { "id": "state-1", "name": "In Progress", "type": "started" }
        }))
        .expect("node should parse");

        assert_eq!(summary.state_category.as_deref(), Some("in_progress"));
        assert_eq!(summary.state_name.as_deref(), Some("In Progress"));
    }

    #[test]
    fn issue_summary_from_node_falls_back_to_state_name_for_category() {
        // No state "type" → category is derived from the state name.
        let summary = issue_summary_from_node(&serde_json::json!({
            "id": "issue-1",
            "title": "Example",
            "state": { "name": "Done" }
        }))
        .expect("node should parse");

        assert_eq!(summary.state_category.as_deref(), Some("done"));
    }

    #[test]
    fn required_trimmed_accepts_trimmed_value_and_rejects_blank() {
        assert_eq!(required_trimmed("  abc  ", "msg").unwrap(), "abc");
        assert_eq!(required_trimmed("   ", "is required").unwrap_err(), "is required");
        assert_eq!(required_trimmed("", "is required").unwrap_err(), "is required");
    }

    #[tokio::test]
    async fn update_issue_state_rejects_empty_inputs_before_network() {
        let client = HyperLinearApiClient::new().expect("client builds");
        let auth = LinearAuthContext {
            api_token: "lin-api-token".to_string(),
        };

        assert_eq!(
            client.update_issue_state(&auth, "  ", "state-1").await.unwrap_err(),
            "Linear issue id is required"
        );
        assert_eq!(
            client.update_issue_state(&auth, "issue-1", "  ").await.unwrap_err(),
            "Linear workflow state id is required"
        );
    }

    #[tokio::test]
    async fn clear_issue_assignee_rejects_empty_issue_id_before_network() {
        let client = HyperLinearApiClient::new().expect("client builds");
        let auth = LinearAuthContext {
            api_token: "lin-api-token".to_string(),
        };

        assert_eq!(
            client.clear_issue_assignee(&auth, "  ").await.unwrap_err(),
            "Linear issue id is required"
        );
    }

    #[tokio::test]
    async fn create_comment_rejects_empty_inputs_before_network() {
        let client = HyperLinearApiClient::new().expect("client builds");
        let auth = LinearAuthContext {
            api_token: "lin-api-token".to_string(),
        };

        assert_eq!(
            client.create_comment(&auth, "  ", "body").await.unwrap_err(),
            "Linear issue id is required"
        );
        assert_eq!(
            client.create_comment(&auth, "issue-1", "   ").await.unwrap_err(),
            "Linear comment body is required"
        );
    }

    #[tokio::test]
    async fn list_issue_team_labels_rejects_empty_issue_id_before_network() {
        let client = HyperLinearApiClient::new().expect("client builds");
        let auth = LinearAuthContext {
            api_token: "lin-api-token".to_string(),
        };

        assert_eq!(
            client.list_issue_team_labels(&auth, "  ").await.unwrap_err(),
            "Linear issue id is required"
        );
    }

    #[tokio::test]
    async fn update_issue_labels_rejects_empty_issue_id_before_network() {
        let client = HyperLinearApiClient::new().expect("client builds");
        let auth = LinearAuthContext {
            api_token: "lin-api-token".to_string(),
        };

        assert_eq!(
            client
                .update_issue_labels(&auth, "  ", vec!["label-1".to_string()])
                .await
                .unwrap_err(),
            "Linear issue id is required"
        );
    }

    #[test]
    fn workflow_states_query_without_team_omits_filter() {
        let (query, variables) = linear_workflow_states_query(None, None);
        assert!(query.contains("workflowStates(first: 100, after: $after)"));
        assert!(!query.contains("filter"));
        assert_eq!(variables, serde_json::json!({ "after": null }));
    }

    #[test]
    fn workflow_states_query_with_team_scopes_filter_and_variables() {
        let (query, variables) = linear_workflow_states_query(Some("  team-1  "), Some("cursor-1"));
        assert!(query.contains("filter: { team: { id: { eq: $teamId } } }"));
        // Team id is trimmed before being placed in the variables payload.
        assert_eq!(
            variables,
            serde_json::json!({ "teamId": "team-1", "after": "cursor-1" })
        );
    }

    #[test]
    fn workflow_states_query_treats_blank_team_as_none() {
        let (query, variables) = linear_workflow_states_query(Some("   "), None);
        assert!(query.contains("workflowStates(first: 100, after: $after)"));
        assert_eq!(variables, serde_json::json!({ "after": null }));
    }
}
