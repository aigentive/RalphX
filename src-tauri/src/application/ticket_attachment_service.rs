use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::clickup_integration_service::ClickUpAttachment;
use super::linear_integration_service::LinearAttachment;
use super::ticketing_identity::{
    normalize_ticket_identity, TicketIdentity, TicketingTicketIdentity, LINK_PROVIDER_JIRA,
    PROVIDER_CLICKUP, PROVIDER_JIRA, PROVIDER_LINEAR,
};
use super::{
    AtlassianIntegrationService, AtlassianJiraAttachment, ClickUpComment,
    ClickUpIntegrationService, LinearIntegrationService,
};
use crate::domain::services::ComposerIntegrationReference;

pub(crate) const MAX_TICKET_ATTACHMENTS: usize = 50;
pub(crate) const MAX_TICKET_ATTACHMENT_TEXT_CHARS: usize = 256;
pub(crate) const MAX_TICKET_ATTACHMENT_MIME_CHARS: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentList {
    pub ticket: TicketingTicketIdentity,
    pub attachments: Vec<TicketAttachmentMetadata>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentMetadata {
    pub provider: String,
    pub ticket_id: String,
    pub ticket_key: Option<String>,
    pub attachment_id: Option<String>,
    pub display_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub author_name: Option<String>,
    pub created_at: Option<String>,
    pub source: TicketAttachmentSource,
    pub retrievable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentSource {
    pub kind: TicketAttachmentSourceKind,
    pub id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketAttachmentSourceKind {
    Ticket,
    Comment,
}

pub struct TicketAttachmentService {
    atlassian: Option<Arc<AtlassianIntegrationService>>,
    linear: Option<Arc<LinearIntegrationService>>,
    clickup: Option<Arc<ClickUpIntegrationService>>,
}

impl TicketAttachmentService {
    pub fn new(
        atlassian: Arc<AtlassianIntegrationService>,
        linear: Arc<LinearIntegrationService>,
        clickup: Arc<ClickUpIntegrationService>,
    ) -> Self {
        Self {
            atlassian: Some(atlassian),
            linear: Some(linear),
            clickup: Some(clickup),
        }
    }

    pub fn with_optional_services(
        atlassian: Option<Arc<AtlassianIntegrationService>>,
        linear: Option<Arc<LinearIntegrationService>>,
        clickup: Option<Arc<ClickUpIntegrationService>>,
    ) -> Self {
        Self {
            atlassian,
            linear,
            clickup,
        }
    }

    pub async fn list_ticket_attachments(
        &self,
        ticket: TicketingTicketIdentity,
    ) -> Result<TicketAttachmentList, String> {
        let identity = normalize_ticket_identity(&ticket)?;
        let mut collector = AttachmentCollector::default();
        match identity.provider.as_str() {
            PROVIDER_JIRA => {
                let atlassian = self
                    .atlassian
                    .as_ref()
                    .ok_or_else(|| "Jira integration service is unavailable".to_string())?;
                let content = atlassian
                    .fetch_resource_content(&composer_reference(
                        LINK_PROVIDER_JIRA,
                        PROVIDER_JIRA,
                        &identity.external_id,
                        identity.external_key.clone(),
                    ))
                    .await?;
                for attachment in &content.attachments {
                    collector.push(jira_attachment_metadata(&identity, attachment));
                }
            }
            PROVIDER_LINEAR => {
                let linear = self
                    .linear
                    .as_ref()
                    .ok_or_else(|| "Linear integration service is unavailable".to_string())?;
                let content = linear
                    .fetch_issue_content(&composer_reference(
                        PROVIDER_LINEAR,
                        PROVIDER_LINEAR,
                        &identity.external_id,
                        identity.external_key.clone(),
                    ))
                    .await?;
                for attachment in &content.attachments {
                    collector.push(linear_attachment_metadata(&identity, attachment));
                }
            }
            PROVIDER_CLICKUP => {
                let clickup = self
                    .clickup
                    .as_ref()
                    .ok_or_else(|| "ClickUp integration service is unavailable".to_string())?;
                let task = clickup.fetch_task(&identity.external_id).await?;
                for attachment in &task.attachments {
                    collector.push(clickup_attachment_metadata(
                        &identity,
                        attachment,
                        TicketAttachmentSourceKind::Ticket,
                        None,
                    ));
                }
                for comment in &task.comments {
                    collect_clickup_comment_attachments(&identity, comment, &mut collector);
                }
            }
            _ => unreachable!("provider validated above"),
        }
        Ok(TicketAttachmentList {
            ticket: normalized_ticket(&identity),
            attachments: collector.attachments,
            truncated: collector.truncated,
        })
    }
}

#[derive(Default)]
struct AttachmentCollector {
    attachments: Vec<TicketAttachmentMetadata>,
    truncated: bool,
}

impl AttachmentCollector {
    fn push(&mut self, attachment: TicketAttachmentMetadata) {
        if self.attachments.len() >= MAX_TICKET_ATTACHMENTS {
            self.truncated = true;
            return;
        }
        self.attachments.push(attachment);
    }
}

fn jira_attachment_metadata(
    identity: &TicketIdentity,
    attachment: &AtlassianJiraAttachment,
) -> TicketAttachmentMetadata {
    TicketAttachmentMetadata {
        provider: identity.provider.clone(),
        ticket_id: ticket_id(identity),
        ticket_key: ticket_key(identity),
        attachment_id: sanitize_optional(&attachment.id, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        display_name: sanitize_required(&attachment.filename, "untitled attachment"),
        mime_type: sanitize_optional(&attachment.mime_type, MAX_TICKET_ATTACHMENT_MIME_CHARS),
        size_bytes: non_negative_size(attachment.size),
        author_name: sanitize_optional(&attachment.author, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        created_at: sanitize_optional(&attachment.created_at, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        source: ticket_source(),
        retrievable: has_value(&attachment.content_url),
    }
}

fn linear_attachment_metadata(
    identity: &TicketIdentity,
    attachment: &LinearAttachment,
) -> TicketAttachmentMetadata {
    TicketAttachmentMetadata {
        provider: identity.provider.clone(),
        ticket_id: ticket_id(identity),
        ticket_key: ticket_key(identity),
        attachment_id: sanitize_optional_string(&attachment.id, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        display_name: sanitize_required(&attachment.title, "untitled attachment"),
        mime_type: None,
        size_bytes: None,
        author_name: sanitize_optional(&attachment.subtitle, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        created_at: None,
        source: ticket_source(),
        retrievable: has_required_value(&attachment.url),
    }
}

fn collect_clickup_comment_attachments(
    identity: &TicketIdentity,
    comment: &ClickUpComment,
    collector: &mut AttachmentCollector,
) {
    let source_id = sanitize_optional_string(&comment.id, MAX_TICKET_ATTACHMENT_TEXT_CHARS);
    for attachment in &comment.attachments {
        collector.push(clickup_attachment_metadata(
            identity,
            attachment,
            TicketAttachmentSourceKind::Comment,
            source_id.clone(),
        ));
    }
    for reply in &comment.replies {
        collect_clickup_comment_attachments(identity, reply, collector);
    }
}

fn clickup_attachment_metadata(
    identity: &TicketIdentity,
    attachment: &ClickUpAttachment,
    source_kind: TicketAttachmentSourceKind,
    source_id: Option<String>,
) -> TicketAttachmentMetadata {
    TicketAttachmentMetadata {
        provider: identity.provider.clone(),
        ticket_id: ticket_id(identity),
        ticket_key: ticket_key(identity),
        attachment_id: sanitize_optional(&attachment.id, MAX_TICKET_ATTACHMENT_TEXT_CHARS),
        display_name: sanitize_required(&attachment.filename, "untitled attachment"),
        mime_type: sanitize_optional(&attachment.mime_type, MAX_TICKET_ATTACHMENT_MIME_CHARS),
        size_bytes: non_negative_size(attachment.size),
        author_name: None,
        created_at: None,
        source: TicketAttachmentSource {
            kind: source_kind,
            id: source_id,
        },
        retrievable: has_value(&attachment.url),
    }
}

fn composer_reference(
    provider: &str,
    kind: &str,
    id: &str,
    key: Option<String>,
) -> ComposerIntegrationReference {
    ComposerIntegrationReference {
        provider: provider.to_string(),
        kind: kind.to_string(),
        id: id.to_string(),
        key,
        title: None,
        url: None,
        summary_excerpt: None,
        include_transcript: None,
    }
}

fn normalized_ticket(identity: &TicketIdentity) -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: identity.provider.clone(),
        id: ticket_id(identity),
        key: ticket_key(identity),
        local_project_id: sanitize_optional(
            &identity.local_project_id,
            MAX_TICKET_ATTACHMENT_TEXT_CHARS,
        ),
    }
}

fn ticket_id(identity: &TicketIdentity) -> String {
    sanitize_required(&identity.external_id, "unknown ticket")
}

fn ticket_key(identity: &TicketIdentity) -> Option<String> {
    sanitize_optional(&identity.external_key, MAX_TICKET_ATTACHMENT_TEXT_CHARS)
}

fn ticket_source() -> TicketAttachmentSource {
    TicketAttachmentSource {
        kind: TicketAttachmentSourceKind::Ticket,
        id: None,
    }
}

fn non_negative_size(size: Option<i64>) -> Option<u64> {
    size.and_then(|value| u64::try_from(value).ok())
}

fn has_value(value: &Option<String>) -> bool {
    value
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
}

fn has_required_value(value: &str) -> bool {
    !value.trim().is_empty()
}

fn sanitize_optional(value: &Option<String>, limit: usize) -> Option<String> {
    value
        .as_deref()
        .and_then(|value| sanitize_optional_string(value, limit))
}

fn sanitize_optional_string(value: &str, limit: usize) -> Option<String> {
    let redacted = redact_external_text(value);
    let trimmed = redacted.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_chars(trimmed, limit))
}

fn sanitize_required(value: &str, fallback: &str) -> String {
    sanitize_optional_string(value, MAX_TICKET_ATTACHMENT_TEXT_CHARS)
        .unwrap_or_else(|| fallback.to_string())
}

fn redact_external_text(value: &str) -> String {
    let mut redact_next = false;
    let mut redacted = Vec::new();
    for token in value.split_whitespace() {
        if redact_next {
            redact_next = false;
            redacted.push("[redacted_secret]");
            continue;
        }
        let lower = token.to_ascii_lowercase();
        if lower.contains("http://") || lower.contains("https://") {
            redacted.push("[redacted_url]");
        } else if lower == "bearer" || lower.starts_with("bearer:") || lower.starts_with("bearer=")
        {
            redact_next = true;
            redacted.push("[redacted_secret]");
        } else if lower.contains("token=")
            || lower.contains("access_token")
            || lower.contains("api_token")
            || lower.contains("secret=")
            || lower.contains("signature=")
        {
            redacted.push("[redacted_secret]");
        } else {
            redacted.push(token);
        }
    }
    redacted.join(" ")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(limit).collect();
    if chars.next().is_none() {
        return prefix;
    }
    let keep = limit.saturating_sub(3);
    let mut truncated = value.chars().take(keep).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
#[path = "ticket_attachment_service_tests.rs"]
mod tests;
