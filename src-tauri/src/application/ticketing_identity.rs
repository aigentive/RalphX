use serde::{Deserialize, Serialize};

pub(crate) const PROVIDER_JIRA: &str = "jira";
pub(crate) const PROVIDER_LINEAR: &str = "linear";
pub(crate) const PROVIDER_CLICKUP: &str = "clickup";
pub(crate) const LINK_PROVIDER_JIRA: &str = "atlassian";
pub(crate) const LINK_PROVIDER_CLICKUP: &str = "clickup";
pub(crate) const KIND_JIRA: &str = "jira";
pub(crate) const KIND_LINEAR: &str = "issue";
pub(crate) const KIND_CLICKUP: &str = "task";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketingTicketIdentity {
    pub provider: String,
    pub id: String,
    pub key: Option<String>,
    pub local_project_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct TicketIdentity {
    pub(crate) provider: String,
    pub(crate) external_kind: String,
    pub(crate) external_id: String,
    pub(crate) external_key: Option<String>,
    pub(crate) local_project_id: Option<String>,
}

pub(crate) fn normalize_ticket_identity(
    ticket: &TicketingTicketIdentity,
) -> Result<TicketIdentity, String> {
    let provider = ticket.provider.trim();
    let provider = match provider {
        PROVIDER_JIRA | PROVIDER_LINEAR | PROVIDER_CLICKUP => provider.to_string(),
        other => return Err(format!("Unknown ticketing provider: {other}")),
    };
    let raw_id = required_trimmed(&ticket.id, "Ticket id is required")?;
    let key = ticket
        .key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    // Only Jira keys its external object by the human-readable issue key; Linear
    // and ClickUp address objects by their opaque id.
    let external_id = if provider == PROVIDER_JIRA {
        key.clone().unwrap_or_else(|| raw_id.to_string())
    } else {
        raw_id.to_string()
    };
    let external_kind = match provider.as_str() {
        PROVIDER_JIRA => KIND_JIRA,
        PROVIDER_CLICKUP => KIND_CLICKUP,
        _ => KIND_LINEAR,
    };
    Ok(TicketIdentity {
        external_kind: external_kind.to_string(),
        provider,
        external_id,
        external_key: key,
        local_project_id: ticket
            .local_project_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    })
}

pub(crate) fn required_trimmed<'a>(value: &'a str, message: &str) -> Result<&'a str, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(message.to_string())
    } else {
        Ok(trimmed)
    }
}
