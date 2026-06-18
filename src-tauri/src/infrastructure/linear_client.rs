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

use crate::application::{
    LinearApiClient, LinearAuthContext, LinearIssueContent, LinearIssueSummary,
};
use crate::domain::services::ComposerIntegrationReference;

const LINEAR_GRAPHQL_ENDPOINT: &str = "https://api.linear.app/graphql";

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
        let data = self
            .graphql(
                &auth.api_token,
                linear_issue_search_query(),
                serde_json::json!({
                    "term": query,
                    "first": limit as i64,
                }),
            )
            .await?;
        search_issue_summaries_from_data(&data)
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
}

fn linear_issue_search_query() -> &'static str {
    r#"
    query RalphXLinearIssueSearch($term: String!, $first: Int!) {
      searchIssues(term: $term, first: $first) {
        nodes {
          id
          identifier
          title
          url
          description
          state {
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

fn search_issue_summaries_from_data(data: &Value) -> Result<Vec<LinearIssueSummary>, String> {
    let nodes = data
        .get("searchIssues")
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
        state_name: node
            .get("state")
            .and_then(|state| state.get("name"))
            .and_then(|value| value.as_str())
            .map(str::to_string),
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

        assert!(query.contains("searchIssues(term: $term, first: $first)"));
        assert!(!query.contains("issues(search:"));
        assert!(!query.contains("issueSearch"));
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
    fn search_issue_summaries_from_data_filters_unreadable_nodes() {
        let data = serde_json::json!({
            "searchIssues": {
                "nodes": [
                    {
                        "id": "issue-1",
                        "identifier": "LIN-1",
                        "title": "Readable",
                        "description": " first issue ",
                        "state": { "name": "Todo" }
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
        assert_eq!(issues[0].state_name.as_deref(), Some("Todo"));
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
                        "state": { "name": "Todo" }
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

        assert_eq!(issues[1].id, "issue-2");
        assert!(issues[1].key.is_none());
        assert_eq!(issues[1].title, "Without optional strings");
        assert!(issues[1].url.is_none());
        assert!(issues[1].excerpt.is_none());
        assert!(issues[1].state_name.is_none());

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
                "updatedAt": "2026-06-18T08:00:00Z"
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
        assert_eq!(content.updated_at.as_deref(), Some("2026-06-18T08:00:00Z"));

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
            "state": { "name": "In Progress" }
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
            "updatedAt": "2026-06-18T08:15:00Z"
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
        assert_eq!(content.updated_at.as_deref(), Some("2026-06-18T08:15:00Z"));
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
}
