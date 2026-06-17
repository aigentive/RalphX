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
            return Err(format!("Linear returned HTTP {}", status.as_u16()));
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

    async fn search_issues(
        &self,
        auth: &LinearAuthContext,
        query: &str,
        limit: usize,
    ) -> Result<Vec<LinearIssueSummary>, String> {
        let data = self
            .graphql(
                &auth.api_token,
                r#"
                query RalphXLinearIssueSearch($query: String!, $first: Int!) {
                  issues(search: $query, first: $first) {
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
                "#,
                serde_json::json!({
                    "query": query,
                    "first": limit as i64,
                }),
            )
            .await?;
        let nodes = data
            .get("issues")
            .and_then(|issues| issues.get("nodes"))
            .and_then(|nodes| nodes.as_array())
            .ok_or_else(|| "Linear search response did not include issues".to_string())?;
        Ok(nodes
            .iter()
            .filter_map(issue_summary_from_node)
            .collect::<Vec<_>>())
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
                    state {
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
        let issue = data
            .get("issue")
            .ok_or_else(|| "Linear issue response did not include issue".to_string())?;
        issue_content_from_node(issue).ok_or_else(|| {
            format!(
                "Linear issue response did not include readable issue {}",
                reference.id
            )
        })
    }
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
    })
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
