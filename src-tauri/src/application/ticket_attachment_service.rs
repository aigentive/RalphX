use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::{
    AtlassianIntegrationService, AtlassianJiraAttachment, ClickUpAttachment, ClickUpComment,
    ClickUpIntegrationService, LinearAttachment, LinearIntegrationService, TicketingTicketIdentity,
};
use crate::domain::services::ComposerIntegrationReference;
use crate::utils::path_safety::validate_absolute_non_root_path;

const PROVIDER_JIRA: &str = "jira";
const PROVIDER_LINEAR: &str = "linear";
const PROVIDER_CLICKUP: &str = "clickup";
const TICKET_ATTACHMENTS_DIR: &str = "ticket_attachments";
const CONTENT_FILE_STEM: &str = "content";
const MAX_ATTACHMENT_FETCH_BYTES: usize = 8 * 1024 * 1024;
const MAX_INLINE_TEXT_BYTES: usize = 64 * 1024;
const UNSAFE_EXTERNAL_LINK_REASON: &str =
    "Attachment external link was withheld because it appears to contain credentials or bearer access material";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentListRequest {
    pub ticket: TicketingTicketIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentFetchRequest {
    pub ticket: TicketingTicketIdentity,
    pub attachment_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentListResponse {
    pub ticket: TicketingTicketIdentity,
    pub attachments: Vec<TicketAttachmentMetadata>,
    pub unsupported_reason: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentFetchResponse {
    pub ticket: TicketingTicketIdentity,
    pub attachment: Option<TicketAttachmentMetadata>,
    pub result: TicketAttachmentFetchResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentMetadata {
    pub provider: String,
    pub ticket: TicketingTicketIdentity,
    pub id: String,
    pub name: String,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub author_name: Option<String>,
    pub created_at: Option<String>,
    pub source: TicketAttachmentSource,
    pub retrieval_kind: TicketAttachmentRetrievalKind,
    pub retrievable: bool,
    pub unsupported_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentSource {
    pub kind: TicketAttachmentSourceKind,
    pub comment_id: Option<String>,
    pub comment_author_name: Option<String>,
    pub comment_created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketAttachmentSourceKind {
    TopLevel,
    Comment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketAttachmentRetrievalKind {
    Download,
    ExternalLink,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TicketAttachmentFetchStatus {
    InlineText,
    CachedFile,
    ExternalLink,
    Unsupported,
    NotFound,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentFetchResult {
    pub status: TicketAttachmentFetchStatus,
    pub inline_text: Option<String>,
    pub cached_file: Option<TicketAttachmentCachedFile>,
    pub external_link: Option<TicketAttachmentExternalLink>,
    pub size: Option<u64>,
    pub sha256: Option<String>,
    pub mime_type: Option<String>,
    pub unsupported_reason: Option<String>,
    pub error_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentCachedFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentExternalLink {
    pub url: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentProviderBytes {
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone)]
struct AttachmentEntry {
    metadata: TicketAttachmentMetadata,
    download_url: Option<String>,
    external_url: Option<String>,
}

pub struct TicketAttachmentService {
    atlassian: Arc<AtlassianIntegrationService>,
    linear: Arc<LinearIntegrationService>,
    clickup: Arc<ClickUpIntegrationService>,
    cache_root: PathBuf,
}

impl TicketAttachmentService {
    pub fn new(
        atlassian: Arc<AtlassianIntegrationService>,
        linear: Arc<LinearIntegrationService>,
        clickup: Arc<ClickUpIntegrationService>,
        cache_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            atlassian,
            linear,
            clickup,
            cache_root: cache_root.as_ref().to_path_buf(),
        }
    }

    pub async fn list_attachments(
        &self,
        request: TicketAttachmentListRequest,
    ) -> Result<TicketAttachmentListResponse, String> {
        let entries = match self.attachment_entries(&request.ticket).await {
            Ok(entries) => entries,
            Err(TicketAttachmentReadError::Unsupported(reason)) => {
                return Ok(TicketAttachmentListResponse {
                    ticket: request.ticket,
                    attachments: Vec::new(),
                    unsupported_reason: Some(reason),
                    error_reason: None,
                });
            }
            Err(TicketAttachmentReadError::Provider(error)) => {
                return Ok(TicketAttachmentListResponse {
                    ticket: request.ticket,
                    attachments: Vec::new(),
                    unsupported_reason: None,
                    error_reason: Some(error),
                });
            }
        };

        Ok(TicketAttachmentListResponse {
            ticket: request.ticket,
            attachments: entries.into_iter().map(|entry| entry.metadata).collect(),
            unsupported_reason: None,
            error_reason: None,
        })
    }

    pub async fn fetch_attachment(
        &self,
        request: TicketAttachmentFetchRequest,
    ) -> Result<TicketAttachmentFetchResponse, String> {
        let entries = match self.attachment_entries(&request.ticket).await {
            Ok(entries) => entries,
            Err(TicketAttachmentReadError::Unsupported(reason)) => {
                return Ok(TicketAttachmentFetchResponse {
                    ticket: request.ticket,
                    attachment: None,
                    result: TicketAttachmentFetchResult::unsupported(reason),
                });
            }
            Err(TicketAttachmentReadError::Provider(error)) => {
                return Ok(TicketAttachmentFetchResponse {
                    ticket: request.ticket,
                    attachment: None,
                    result: TicketAttachmentFetchResult::error(error),
                });
            }
        };

        let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.metadata.id == request.attachment_id)
        else {
            return Ok(TicketAttachmentFetchResponse {
                ticket: request.ticket,
                attachment: None,
                result: TicketAttachmentFetchResult::not_found(format!(
                    "Ticket attachment not found: {}",
                    request.attachment_id
                )),
            });
        };

        let result = match entry.metadata.retrieval_kind {
            TicketAttachmentRetrievalKind::ExternalLink => match entry.external_url.as_deref() {
                Some(url) => match safe_agent_external_url(url) {
                    Ok(url) => TicketAttachmentFetchResult::external_link(
                        url,
                        Some(entry.metadata.name.clone()),
                    ),
                    Err(reason) => TicketAttachmentFetchResult::unsupported(reason),
                },
                _ => TicketAttachmentFetchResult::unsupported(
                    "Attachment external link is unavailable".to_string(),
                ),
            },
            TicketAttachmentRetrievalKind::Download => {
                self.fetch_downloadable_attachment(&entry).await?
            }
            TicketAttachmentRetrievalKind::Unsupported => {
                TicketAttachmentFetchResult::unsupported(
                    entry
                        .metadata
                        .unsupported_reason
                        .clone()
                        .unwrap_or_else(|| "Attachment is not retrievable".to_string()),
                )
            }
        };

        Ok(TicketAttachmentFetchResponse {
            ticket: request.ticket,
            attachment: Some(entry.metadata),
            result,
        })
    }

    async fn attachment_entries(
        &self,
        ticket: &TicketingTicketIdentity,
    ) -> Result<Vec<AttachmentEntry>, TicketAttachmentReadError> {
        match normalized_provider(&ticket.provider).as_str() {
            PROVIDER_JIRA => self.jira_attachment_entries(ticket).await,
            PROVIDER_LINEAR => self.linear_attachment_entries(ticket).await,
            PROVIDER_CLICKUP => self.clickup_attachment_entries(ticket).await,
            provider => Err(TicketAttachmentReadError::Unsupported(format!(
                "Unsupported ticket attachment provider: {provider}"
            ))),
        }
    }

    async fn jira_attachment_entries(
        &self,
        ticket: &TicketingTicketIdentity,
    ) -> Result<Vec<AttachmentEntry>, TicketAttachmentReadError> {
        let issue_id = ticket.key.as_deref().unwrap_or(&ticket.id).trim();
        if issue_id.is_empty() {
            return Err(TicketAttachmentReadError::Provider(
                "Jira ticket id or key is required".to_string(),
            ));
        }
        let reference = ComposerIntegrationReference {
            provider: "atlassian".to_string(),
            kind: "jira".to_string(),
            id: issue_id.to_string(),
            key: ticket.key.clone(),
            title: None,
            url: None,
            summary_excerpt: None,
            include_transcript: None,
        };
        let content = self
            .atlassian
            .fetch_resource_content(&reference)
            .await
            .map_err(TicketAttachmentReadError::Provider)?;
        Ok(content
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| jira_entry(ticket, attachment, index))
            .collect())
    }

    async fn linear_attachment_entries(
        &self,
        ticket: &TicketingTicketIdentity,
    ) -> Result<Vec<AttachmentEntry>, TicketAttachmentReadError> {
        let issue_id = ticket.id.trim();
        if issue_id.is_empty() {
            return Err(TicketAttachmentReadError::Provider(
                "Linear ticket id is required".to_string(),
            ));
        }
        let reference = ComposerIntegrationReference {
            provider: "linear".to_string(),
            kind: "issue".to_string(),
            id: issue_id.to_string(),
            key: ticket.key.clone(),
            title: None,
            url: None,
            summary_excerpt: None,
            include_transcript: None,
        };
        let content = self
            .linear
            .fetch_issue_content(&reference)
            .await
            .map_err(TicketAttachmentReadError::Provider)?;
        Ok(content
            .attachments
            .iter()
            .map(|attachment| linear_entry(ticket, attachment))
            .collect())
    }

    async fn clickup_attachment_entries(
        &self,
        ticket: &TicketingTicketIdentity,
    ) -> Result<Vec<AttachmentEntry>, TicketAttachmentReadError> {
        let task_id = ticket.id.trim();
        if task_id.is_empty() {
            return Err(TicketAttachmentReadError::Provider(
                "ClickUp ticket id is required".to_string(),
            ));
        }
        let task = self
            .clickup
            .fetch_task(task_id)
            .await
            .map_err(TicketAttachmentReadError::Provider)?;
        let mut entries = Vec::new();
        entries.extend(
            task.attachments
                .iter()
                .enumerate()
                .map(|(index, attachment)| clickup_top_level_entry(ticket, attachment, index)),
        );
        for comment in &task.comments {
            collect_clickup_comment_entries(ticket, comment, &mut entries);
        }
        Ok(entries)
    }

    async fn fetch_downloadable_attachment(
        &self,
        entry: &AttachmentEntry,
    ) -> Result<TicketAttachmentFetchResult, String> {
        let Some(download_url) = entry.download_url.as_deref() else {
            return Ok(TicketAttachmentFetchResult::unsupported(
                "Attachment download URL is unavailable".to_string(),
            ));
        };
        let provider_bytes = match entry.metadata.provider.as_str() {
            PROVIDER_JIRA => {
                self.atlassian
                    .fetch_jira_attachment_bytes(download_url, MAX_ATTACHMENT_FETCH_BYTES)
                    .await
            }
            PROVIDER_CLICKUP => {
                self.clickup
                    .fetch_attachment_bytes(download_url, MAX_ATTACHMENT_FETCH_BYTES)
                    .await
            }
            _ => Err("Attachment downloads are not supported for this provider".to_string()),
        };
        let provider_bytes = match provider_bytes {
            Ok(bytes) => bytes,
            Err(error) => return Ok(TicketAttachmentFetchResult::error(error)),
        };
        let mime_type = provider_bytes
            .mime_type
            .clone()
            .or_else(|| entry.metadata.mime_type.clone());
        let size = provider_bytes.bytes.len() as u64;
        let sha256 = sha256_hex(&provider_bytes.bytes);

        if provider_bytes.bytes.len() <= MAX_INLINE_TEXT_BYTES
            && is_safe_text_mime(mime_type.as_deref())
        {
            match String::from_utf8(provider_bytes.bytes) {
                Ok(text) => {
                    return Ok(TicketAttachmentFetchResult {
                        status: TicketAttachmentFetchStatus::InlineText,
                        inline_text: Some(text),
                        cached_file: None,
                        external_link: None,
                        size: Some(size),
                        sha256: Some(sha256),
                        mime_type,
                        unsupported_reason: None,
                        error_reason: None,
                    });
                }
                Err(error) => {
                    let bytes = error.into_bytes();
                    return self.cache_binary_attachment(entry, bytes, mime_type, sha256).await;
                }
            }
        }

        self.cache_binary_attachment(entry, provider_bytes.bytes, mime_type, sha256)
            .await
    }

    async fn cache_binary_attachment(
        &self,
        entry: &AttachmentEntry,
        bytes: Vec<u8>,
        mime_type: Option<String>,
        sha256: String,
    ) -> Result<TicketAttachmentFetchResult, String> {
        let base = validate_absolute_non_root_path(&self.cache_root, "ticket attachment cache")
            .map_err(|error| error.to_string())?;
        let path = build_ticket_attachment_file_path(&base, &entry.metadata, &sha256);
        let safe_path = validate_absolute_non_root_path(&path, "ticket attachment cache file")
            .map_err(|error| error.to_string())?;
        if !safe_path.starts_with(&base) {
            return Err("Ticket attachment cache path escaped the cache root".to_string());
        }
        let parent = safe_path
            .parent()
            .ok_or_else(|| "Ticket attachment cache file is missing a parent path".to_string())?;
        // codeql[rust/path-injection]: parent is derived from validated cache root plus hash-only components and containment-checked above.
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| {
                format!(
                    "Failed to create ticket attachment cache directory {}: {error}",
                    parent.display()
                )
            })?;
        // codeql[rust/path-injection]: safe_path is derived from validated cache root plus hash-only components and containment-checked above.
        tokio::fs::write(&safe_path, &bytes).await.map_err(|error| {
            format!(
                "Failed to write ticket attachment cache file {}: {error}",
                safe_path.display()
            )
        })?;
        let size = bytes.len() as u64;
        Ok(TicketAttachmentFetchResult {
            status: TicketAttachmentFetchStatus::CachedFile,
            inline_text: None,
            cached_file: Some(TicketAttachmentCachedFile {
                path: safe_path.to_string_lossy().to_string(),
                size,
                sha256: sha256.clone(),
                mime_type: mime_type.clone(),
            }),
            external_link: None,
            size: Some(size),
            sha256: Some(sha256),
            mime_type,
            unsupported_reason: None,
            error_reason: None,
        })
    }
}

impl TicketAttachmentFetchResult {
    fn unsupported(reason: String) -> Self {
        Self {
            status: TicketAttachmentFetchStatus::Unsupported,
            inline_text: None,
            cached_file: None,
            external_link: None,
            size: None,
            sha256: None,
            mime_type: None,
            unsupported_reason: Some(reason),
            error_reason: None,
        }
    }

    fn error(reason: String) -> Self {
        Self {
            status: TicketAttachmentFetchStatus::Error,
            inline_text: None,
            cached_file: None,
            external_link: None,
            size: None,
            sha256: None,
            mime_type: None,
            unsupported_reason: None,
            error_reason: Some(reason),
        }
    }

    fn not_found(reason: String) -> Self {
        Self {
            status: TicketAttachmentFetchStatus::NotFound,
            inline_text: None,
            cached_file: None,
            external_link: None,
            size: None,
            sha256: None,
            mime_type: None,
            unsupported_reason: Some(reason),
            error_reason: None,
        }
    }

    fn external_link(url: String, title: Option<String>) -> Self {
        Self {
            status: TicketAttachmentFetchStatus::ExternalLink,
            inline_text: None,
            cached_file: None,
            external_link: Some(TicketAttachmentExternalLink { url, title }),
            size: None,
            sha256: None,
            mime_type: None,
            unsupported_reason: None,
            error_reason: None,
        }
    }
}

#[derive(Debug)]
enum TicketAttachmentReadError {
    Unsupported(String),
    Provider(String),
}

fn jira_entry(
    ticket: &TicketingTicketIdentity,
    attachment: &AtlassianJiraAttachment,
    index: usize,
) -> AttachmentEntry {
    let id = attachment
        .id
        .clone()
        .unwrap_or_else(|| derived_attachment_id("jira", &attachment.filename, index));
    let has_download = attachment
        .content_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    let unsupported_reason = if has_download {
        None
    } else {
        Some("Jira attachment has no downloadable content URL".to_string())
    };
    let metadata = TicketAttachmentMetadata {
        provider: PROVIDER_JIRA.to_string(),
        ticket: normalized_ticket(ticket, PROVIDER_JIRA),
        id,
        name: attachment.filename.clone(),
        mime_type: attachment.mime_type.clone(),
        size: attachment.size,
        author_name: attachment.author.clone(),
        created_at: attachment.created_at.clone(),
        source: top_level_source(),
        retrieval_kind: if has_download {
            TicketAttachmentRetrievalKind::Download
        } else {
            TicketAttachmentRetrievalKind::Unsupported
        },
        retrievable: has_download,
        unsupported_reason,
    };
    AttachmentEntry {
        metadata,
        download_url: attachment.content_url.clone(),
        external_url: None,
    }
}

fn linear_entry(ticket: &TicketingTicketIdentity, attachment: &LinearAttachment) -> AttachmentEntry {
    let has_link = !attachment.url.trim().is_empty();
    let metadata = TicketAttachmentMetadata {
        provider: PROVIDER_LINEAR.to_string(),
        ticket: normalized_ticket(ticket, PROVIDER_LINEAR),
        id: attachment.id.clone(),
        name: attachment.title.clone(),
        mime_type: None,
        size: None,
        author_name: None,
        created_at: None,
        source: top_level_source(),
        retrieval_kind: if has_link {
            TicketAttachmentRetrievalKind::ExternalLink
        } else {
            TicketAttachmentRetrievalKind::Unsupported
        },
        retrievable: has_link,
        unsupported_reason: if has_link {
            None
        } else {
            Some("Linear attachment link is unavailable".to_string())
        },
    };
    AttachmentEntry {
        metadata,
        download_url: None,
        external_url: if has_link {
            Some(attachment.url.clone())
        } else {
            None
        },
    }
}

fn clickup_top_level_entry(
    ticket: &TicketingTicketIdentity,
    attachment: &ClickUpAttachment,
    index: usize,
) -> AttachmentEntry {
    clickup_entry(ticket, attachment, top_level_source(), index)
}

fn clickup_entry(
    ticket: &TicketingTicketIdentity,
    attachment: &ClickUpAttachment,
    source: TicketAttachmentSource,
    index: usize,
) -> AttachmentEntry {
    let id = attachment
        .id
        .clone()
        .unwrap_or_else(|| derived_attachment_id("clickup", &attachment.filename, index));
    let has_download = attachment
        .url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    let metadata = TicketAttachmentMetadata {
        provider: PROVIDER_CLICKUP.to_string(),
        ticket: normalized_ticket(ticket, PROVIDER_CLICKUP),
        id,
        name: attachment.filename.clone(),
        mime_type: attachment.mime_type.clone(),
        size: attachment.size,
        author_name: None,
        created_at: None,
        source,
        retrieval_kind: if has_download {
            TicketAttachmentRetrievalKind::Download
        } else {
            TicketAttachmentRetrievalKind::Unsupported
        },
        retrievable: has_download,
        unsupported_reason: if has_download {
            None
        } else {
            Some("ClickUp attachment has no downloadable URL".to_string())
        },
    };
    AttachmentEntry {
        metadata,
        download_url: attachment.url.clone(),
        external_url: None,
    }
}

fn collect_clickup_comment_entries(
    ticket: &TicketingTicketIdentity,
    comment: &ClickUpComment,
    entries: &mut Vec<AttachmentEntry>,
) {
    let source = TicketAttachmentSource {
        kind: TicketAttachmentSourceKind::Comment,
        comment_id: Some(comment.id.clone()),
        comment_author_name: comment.author_name.clone(),
        comment_created_at: comment.created_at.clone(),
    };
    entries.extend(
        comment
            .attachments
            .iter()
            .enumerate()
            .map(|(index, attachment)| clickup_entry(ticket, attachment, source.clone(), index)),
    );
    for reply in &comment.replies {
        collect_clickup_comment_entries(ticket, reply, entries);
    }
}

fn top_level_source() -> TicketAttachmentSource {
    TicketAttachmentSource {
        kind: TicketAttachmentSourceKind::TopLevel,
        comment_id: None,
        comment_author_name: None,
        comment_created_at: None,
    }
}

fn normalized_ticket(
    ticket: &TicketingTicketIdentity,
    provider: &str,
) -> TicketingTicketIdentity {
    TicketingTicketIdentity {
        provider: provider.to_string(),
        id: ticket.id.trim().to_string(),
        key: ticket
            .key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        local_project_id: ticket
            .local_project_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    }
}

fn normalized_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase()
}

fn safe_agent_external_url(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("Attachment external link is unavailable".to_string());
    }

    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(
            "Attachment external link was withheld because only HTTP(S) links are supported"
                .to_string(),
        );
    }
    if url_contains_credentials(&lower) || url_contains_sensitive_material(trimmed) {
        return Err(UNSAFE_EXTERNAL_LINK_REASON.to_string());
    }

    Ok(trimmed.to_string())
}

fn url_contains_credentials(lower_url: &str) -> bool {
    let without_scheme = lower_url
        .strip_prefix("https://")
        .or_else(|| lower_url.strip_prefix("http://"))
        .unwrap_or(lower_url);
    without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .contains('@')
}

fn url_contains_sensitive_material(url: &str) -> bool {
    if crate::utils::secret_redactor::redact(url) != url {
        return true;
    }

    let lower = url.to_ascii_lowercase();
    if lower.contains("bearer%20") || lower.contains("authorization%3a") {
        return true;
    }

    lower
        .split(['?', '#'])
        .skip(1)
        .flat_map(|tail| tail.split(['&', ';', '#']))
        .filter_map(|pair| pair.split_once('=').map(|(key, _)| key.trim()))
        .any(is_sensitive_url_param)
}

fn is_sensitive_url_param(key: &str) -> bool {
    matches!(
        key,
        "authorization" | "auth" | "bearer" | "jwt" | "key" | "sig" | "signature"
    ) || key.contains("token")
        || key.contains("secret")
        || key.ends_with("_key")
        || key.ends_with("apikey")
}

fn derived_attachment_id(provider: &str, name: &str, index: usize) -> String {
    format!("{provider}-{}", stable_hash(&format!("{name}:{index}")))
}

fn build_ticket_attachment_file_path(
    base: &Path,
    metadata: &TicketAttachmentMetadata,
    sha256: &str,
) -> PathBuf {
    let content_file_name = attachment_content_file_name(&metadata.name);
    base
        .join(TICKET_ATTACHMENTS_DIR)
        .join(hashed_component("provider", &metadata.provider))
        .join(hashed_component("ticket", &ticket_path_key(&metadata.ticket)))
        .join(hashed_component("attachment", &metadata.id))
        .join(hashed_component("content", sha256))
        .join(content_file_name)
}

fn ticket_path_key(ticket: &TicketingTicketIdentity) -> String {
    format!(
        "{}:{}:{}:{}",
        ticket.provider,
        ticket.id,
        ticket.key.as_deref().unwrap_or_default(),
        ticket.local_project_id.as_deref().unwrap_or_default()
    )
}

fn attachment_content_file_name(file_name: &str) -> String {
    let extension = safe_attachment_extension(file_name);

    match extension {
        Some(extension) => format!("{CONTENT_FILE_STEM}.{extension}"),
        None => CONTENT_FILE_STEM.to_string(),
    }
}

fn safe_attachment_extension(file_name: &str) -> Option<String> {
    if file_name.is_empty() || file_name.contains('/') || file_name.contains('\\') {
        return None;
    }
    let mut components = Path::new(file_name).components();
    if !matches!((components.next(), components.next()), (Some(Component::Normal(_)), None)) {
        return None;
    }

    Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| is_safe_extension(extension))
}

fn is_safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_safe_text_mime(mime_type: Option<&str>) -> bool {
    let Some(mime_type) = mime_type else {
        return false;
    };
    let normalized = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    normalized.starts_with("text/")
        || matches!(
            normalized.as_str(),
            "application/json"
                | "application/xml"
                | "application/x-yaml"
                | "application/yaml"
                | "application/javascript"
                | "application/typescript"
                | "application/toml"
        )
        || normalized.ends_with("+json")
        || normalized.ends_with("+xml")
}

fn hashed_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(24);
    for byte in &digest[..12] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    format!("{prefix}-{encoded}")
}

fn stable_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex_prefix(&digest, 12)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_prefix(&digest, digest.len())
}

fn hex_prefix(bytes: &[u8], len: usize) -> String {
    let mut encoded = String::with_capacity(len * 2);
    for byte in &bytes[..len] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}
