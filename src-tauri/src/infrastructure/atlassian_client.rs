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

use crate::application::{
    AtlassianApiClient, AtlassianAuthContext, AtlassianConnectivity, AtlassianCredential,
    AtlassianOAuthResource, AtlassianOAuthTokenResponse, AtlassianResourceContent,
    AtlassianResourceKind, AtlassianResourceSummary,
};
use crate::domain::services::ComposerIntegrationReference;

pub struct HyperAtlassianApiClient {
    client: Client<HttpsConnector<HttpConnector>, Full<Bytes>>,
    timeout: Duration,
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
        })
    }

    fn resource_url(
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

    async fn request_json(
        &self,
        method: Method,
        url: String,
        auth: RequestAuth<'_>,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let uri = url
            .parse::<hyper::Uri>()
            .map_err(|error| format!("Invalid Atlassian URL: {error}"))?;
        let body_bytes = body
            .map(|value| serde_json::to_vec(&value))
            .transpose()
            .map_err(|error| error.to_string())?
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
            .map_err(|error| format!("Failed to build Atlassian request: {error}"))?;
        let response = tokio::time::timeout(self.timeout, self.client.request(request))
            .await
            .map_err(|_| "Atlassian request timed out".to_string())?
            .map_err(|error| format!("Atlassian request failed: {error}"))?;
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .map_err(|error| format!("Failed to read Atlassian response: {error}"))?
            .to_bytes();
        if !status.is_success() {
            return Err(format!("Atlassian returned HTTP {}", status.as_u16()));
        }
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_slice(&bytes)
            .map_err(|error| format!("Failed to parse Atlassian response: {error}"))
    }
}

fn install_rustls_crypto_provider() {
    static INSTALL_PROVIDER: std::sync::Once = std::sync::Once::new();
    INSTALL_PROVIDER.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

enum RequestAuth<'a> {
    Basic { email: &'a str, token: &'a str },
    Bearer(&'a str),
    None,
}

fn request_auth(auth: &AtlassianAuthContext) -> RequestAuth<'_> {
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

    async fn fetch(
        &self,
        auth: &AtlassianAuthContext,
        reference: &ComposerIntegrationReference,
    ) -> Result<AtlassianResourceContent, String> {
        match reference.kind.as_str() {
            "jira" => fetch_jira(self, auth, reference).await,
            "confluence" => fetch_confluence(self, auth, reference).await,
            other => Err(format!("Unsupported Atlassian reference kind: {other}")),
        }
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

async fn search_jira(
    client: &HyperAtlassianApiClient,
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
    let mut results = Vec::new();
    for issue in value
        .get("sections")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|section| section.get("issues").and_then(Value::as_array))
        .flatten()
    {
        let Some(key) = issue.get("key").and_then(Value::as_str) else {
            continue;
        };
        let title = issue
            .get("summaryText")
            .or_else(|| issue.get("summary"))
            .and_then(Value::as_str)
            .unwrap_or(key)
            .to_string();
        results.push(AtlassianResourceSummary {
            kind: AtlassianResourceKind::Jira,
            id: key.to_string(),
            key: Some(key.to_string()),
            title,
            url: Some(format!("{site_url}/browse/{key}")),
            excerpt: None,
        });
        if results.len() >= limit {
            break;
        }
    }
    Ok(results)
}

async fn oauth_token_request(
    client: &HyperAtlassianApiClient,
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

async fn search_confluence(
    client: &HyperAtlassianApiClient,
    auth: &AtlassianAuthContext,
    query: &str,
    limit: usize,
) -> Result<Vec<AtlassianResourceSummary>, String> {
    let cql = format!(
        "type=page AND text ~ \"{}*\"",
        query.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let value = client
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
        .await?;
    let site_url = &auth.site_url;
    Ok(value
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|result| {
            let content = result.get("content")?;
            let id = content.get("id").and_then(Value::as_str)?;
            let title = content.get("title").and_then(Value::as_str).unwrap_or(id);
            let webui = content
                .get("_links")
                .and_then(|links| links.get("webui"))
                .and_then(Value::as_str);
            Some(AtlassianResourceSummary {
                kind: AtlassianResourceKind::Confluence,
                id: id.to_string(),
                key: None,
                title: title.to_string(),
                url: webui.map(|path| format!("{site_url}/wiki{path}")),
                excerpt: result
                    .get("excerpt")
                    .and_then(Value::as_str)
                    .map(strip_html),
            })
        })
        .take(limit)
        .collect())
}

async fn fetch_jira(
    client: &HyperAtlassianApiClient,
    auth: &AtlassianAuthContext,
    reference: &ComposerIntegrationReference,
) -> Result<AtlassianResourceContent, String> {
    let key = reference.key.as_deref().unwrap_or(&reference.id);
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                &format!(
                    "/rest/api/3/issue/{}?fields=summary,status,description,assignee,reporter,updated,comment",
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
    let mut body = Vec::new();
    push_field(&mut body, "Key", key);
    push_field(&mut body, "Summary", &title);
    if let Some(status) = fields
        .get("status")
        .and_then(|status| status.get("name"))
        .and_then(Value::as_str)
    {
        push_field(&mut body, "Status", status);
    }
    if let Some(updated) = fields.get("updated").and_then(Value::as_str) {
        push_field(&mut body, "Updated", updated);
    }
    if let Some(description) = fields.get("description").filter(|value| !value.is_null()) {
        body.push("Description:".to_string());
        body.push(render_jsonish(description));
    }
    if let Some(comments) = fields
        .get("comment")
        .and_then(|comment| comment.get("comments"))
        .and_then(Value::as_array)
    {
        for comment in comments.iter().rev().take(3).rev() {
            if let Some(content) = comment.get("body") {
                body.push("Comment:".to_string());
                body.push(render_jsonish(content));
            }
        }
    }
    Ok(AtlassianResourceContent {
        kind: AtlassianResourceKind::Jira,
        id: key.to_string(),
        key: Some(key.to_string()),
        title,
        url: Some(format!("{site_url}/browse/{key}")),
        body: body.join("\n"),
    })
}

async fn fetch_confluence(
    client: &HyperAtlassianApiClient,
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
    Ok(AtlassianResourceContent {
        kind: AtlassianResourceKind::Confluence,
        id: reference.id.clone(),
        key: None,
        title,
        url,
        body,
    })
}

fn push_field(lines: &mut Vec<String>, label: &str, value: &str) {
    if !value.trim().is_empty() {
        lines.push(format!("{label}: {}", value.trim()));
    }
}

fn render_jsonish(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default())
}

fn strip_html(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructing_client_does_not_panic_when_roots_are_unavailable() {
        let result = std::panic::catch_unwind(HyperAtlassianApiClient::new);
        assert!(result.is_ok());
    }
}
