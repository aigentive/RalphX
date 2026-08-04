use std::collections::HashMap;

use hyper::Method;
use serde_json::Value;
use tokio::sync::RwLock;

use super::atlassian_client::{
    jira_rich_text, request_auth, AtlassianAuthContext, AtlassianCredential,
    AtlassianJsonRequester, AtlassianResourceKind, HyperAtlassianApiClient, JiraRichText,
};

const ACCEPTANCE_CRITERIA_FIELD_NAMES: [&str; 4] = [
    "acceptance criteria",
    "acceptance criterias",
    "acceptance criterion",
    "acceptance criteria ac",
];
const MAX_ACCEPTANCE_CRITERIA_FIELDS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct JiraFieldDescriptor {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) custom: bool,
}

pub(crate) async fn fetch_jira_field_catalog<C: AtlassianJsonRequester + ?Sized>(
    client: &C,
    auth: &AtlassianAuthContext,
) -> Result<Vec<JiraFieldDescriptor>, String> {
    let value = client
        .request_json(
            Method::GET,
            HyperAtlassianApiClient::resource_url(
                auth,
                AtlassianResourceKind::Jira,
                "/rest/api/3/field",
            ),
            request_auth(auth),
            None,
        )
        .await?;
    let fields = value
        .as_array()
        .ok_or_else(|| "Jira field catalog response was not an array".to_string())?;

    Ok(fields
        .iter()
        .filter_map(|field| {
            let id = field.get("id").and_then(Value::as_str)?.trim();
            let name = field.get("name").and_then(Value::as_str)?.trim();
            if id.is_empty() || name.is_empty() {
                return None;
            }
            Some(JiraFieldDescriptor {
                id: id.to_string(),
                name: name.to_string(),
                custom: field
                    .get("custom")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            })
        })
        .collect())
}

pub(crate) fn acceptance_criteria_field_ids(fields: &[JiraFieldDescriptor]) -> Vec<String> {
    let mut matches = fields
        .iter()
        .filter_map(|field| {
            let normalized = normalize_field_name(&field.name);
            ACCEPTANCE_CRITERIA_FIELD_NAMES
                .contains(&normalized.as_str())
                .then(|| {
                    let exact = field
                        .name
                        .trim()
                        .eq_ignore_ascii_case("acceptance criteria");
                    (exact, field.id.clone())
                })
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    matches
        .into_iter()
        .take(MAX_ACCEPTANCE_CRITERIA_FIELDS)
        .map(|(_, id)| id)
        .collect()
}

pub(crate) fn acceptance_criteria_from_fields(
    fields: &Value,
    field_ids: &[String],
) -> Option<JiraRichText> {
    field_ids.iter().find_map(|field_id| {
        let value = fields.get(field_id)?;
        if value.is_null() {
            return None;
        }
        let normalized;
        let rich_value = if let Some(values) = value.as_array() {
            normalized = values
                .iter()
                .filter_map(option_display_text)
                .map(|value| format!("- {value}"))
                .collect::<Vec<_>>()
                .join("\n");
            Value::String(normalized)
        } else {
            value.clone()
        };
        let rich_text = jira_rich_text(&rich_value);
        (!rich_text.markdown.trim().is_empty() || !rich_text.text.trim().is_empty())
            .then_some(rich_text)
    })
}

pub(crate) struct JiraFieldCatalogCache {
    field_ids: RwLock<HashMap<String, Vec<String>>>,
}

impl JiraFieldCatalogCache {
    pub(crate) fn new() -> Self {
        Self {
            field_ids: RwLock::new(HashMap::new()),
        }
    }

    pub(crate) async fn acceptance_criteria_ids<C: AtlassianJsonRequester + ?Sized>(
        &self,
        client: &C,
        auth: &AtlassianAuthContext,
    ) -> Vec<String> {
        let cache_key = jira_site_cache_key(auth);
        if let Some(ids) = self.field_ids.read().await.get(&cache_key).cloned() {
            return ids;
        }

        match fetch_jira_field_catalog(client, auth).await {
            Ok(fields) => {
                let ids = acceptance_criteria_field_ids(&fields);
                self.field_ids.write().await.insert(cache_key, ids.clone());
                ids
            }
            Err(error) => {
                tracing::debug!(
                    site_url = %auth.site_url,
                    error = %error,
                    "Unable to discover Jira acceptance criteria fields"
                );
                Vec::new()
            }
        }
    }
}

fn normalize_field_name(value: &str) -> String {
    let collapsed = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let without_colon = collapsed.trim_end_matches(':').trim_end();
    without_colon
        .strip_suffix("(s)")
        .unwrap_or(without_colon)
        .trim_end()
        .to_string()
}

fn option_display_text(value: &Value) -> Option<String> {
    let display = value
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            ["value", "name", "text"]
                .iter()
                .find_map(|key| value.get(key).and_then(Value::as_str).map(str::to_string))
        })
        .unwrap_or_else(|| value.to_string());
    let display = display.trim();
    (!display.is_empty() && display != "null").then(|| display.to_string())
}

fn jira_site_cache_key(auth: &AtlassianAuthContext) -> String {
    let cloud_id = match &auth.credential {
        AtlassianCredential::ApiToken { .. } => "",
        AtlassianCredential::OAuth { cloud_id, .. } => cloud_id,
    };
    format!("{}|{cloud_id}", auth.site_url)
}
