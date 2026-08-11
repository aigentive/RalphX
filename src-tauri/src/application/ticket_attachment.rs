use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const MAX_TICKET_ATTACHMENT_LIST_ITEMS: usize = 100;
pub const MAX_TICKET_ATTACHMENT_CONTENT_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TICKET_ATTACHMENT_PERSISTED_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_TICKET_ATTACHMENT_ID_BYTES: usize = 512;
pub const MAX_TICKET_ATTACHMENT_TICKET_ID_BYTES: usize = 512;
pub const MAX_TICKET_ATTACHMENT_FILE_NAME_BYTES: usize = 512;
pub const MAX_TICKET_ATTACHMENT_MEDIA_TYPE_BYTES: usize = 255;

const TICKET_ATTACHMENTS_DIR: &str = "ticket_attachments";
const CONTENT_FILE_STEM: &str = "content";
const POINTER_PREFIX: &str = "ta";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TicketAttachmentProvider {
    Jira,
    Linear,
    ClickUp,
}

impl TicketAttachmentProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Linear => "linear",
            Self::ClickUp => "clickup",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentContentPointer {
    id: String,
}

impl TicketAttachmentContentPointer {
    pub fn from_id(id: &str) -> Result<Self, TicketAttachmentError> {
        validate_identifier("content_pointer", id, MAX_TICKET_ATTACHMENT_ID_BYTES)?;
        if !id.starts_with("ta_") {
            return Err(TicketAttachmentError::UnsafeField {
                field: "content_pointer",
            });
        }
        Ok(Self { id: id.to_string() })
    }

    pub fn new(
        provider: TicketAttachmentProvider,
        ticket_id: &str,
        attachment_id: &str,
    ) -> Result<Self, TicketAttachmentError> {
        validate_identifier(
            "ticket_id",
            ticket_id,
            MAX_TICKET_ATTACHMENT_TICKET_ID_BYTES,
        )?;
        validate_identifier(
            "attachment_id",
            attachment_id,
            MAX_TICKET_ATTACHMENT_ID_BYTES,
        )?;

        let mut hasher = Sha256::new();
        hasher.update(b"ralphx-ticket-attachment-pointer-v1");
        hasher.update(b"\0");
        hasher.update(provider.as_str().as_bytes());
        hasher.update(b"\0");
        hasher.update(ticket_id.as_bytes());
        hasher.update(b"\0");
        hasher.update(attachment_id.as_bytes());
        let digest = hasher.finalize();

        Ok(Self {
            id: format!("{POINTER_PREFIX}_{}", hex_prefix(&digest, 24)),
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketAttachmentDescriptor {
    pub provider: TicketAttachmentProvider,
    pub id: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    pub content_pointer: TicketAttachmentContentPointer,
}

impl TicketAttachmentDescriptor {
    pub fn new(
        provider: TicketAttachmentProvider,
        ticket_id: &str,
        attachment_id: &str,
        file_name: &str,
        media_type: Option<&str>,
        declared_size_bytes: Option<u64>,
        created_at: Option<String>,
    ) -> Result<Self, TicketAttachmentError> {
        validate_identifier(
            "attachment_id",
            attachment_id,
            MAX_TICKET_ATTACHMENT_ID_BYTES,
        )?;
        validate_display_value(
            "file_name",
            file_name,
            MAX_TICKET_ATTACHMENT_FILE_NAME_BYTES,
        )?;
        if let Some(media_type) = media_type {
            validate_display_value(
                "media_type",
                media_type,
                MAX_TICKET_ATTACHMENT_MEDIA_TYPE_BYTES,
            )?;
        }
        if let Some(size) = declared_size_bytes {
            ensure_ticket_attachment_content_size(size)?;
        }

        let content_pointer =
            TicketAttachmentContentPointer::new(provider, ticket_id, attachment_id)?;

        Ok(Self {
            provider,
            id: content_pointer.id().to_string(),
            file_name: file_name.to_string(),
            media_type: media_type.map(ToString::to_string),
            declared_size_bytes,
            created_at,
            content_pointer,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachmentSourceHandle {
    provider: TicketAttachmentProvider,
    ticket_id: String,
    attachment_id: String,
    fetch_url: Option<String>,
}

impl TicketAttachmentSourceHandle {
    pub fn new(
        provider: TicketAttachmentProvider,
        ticket_id: &str,
        attachment_id: &str,
    ) -> Result<Self, TicketAttachmentError> {
        validate_identifier(
            "ticket_id",
            ticket_id,
            MAX_TICKET_ATTACHMENT_TICKET_ID_BYTES,
        )?;
        validate_identifier(
            "attachment_id",
            attachment_id,
            MAX_TICKET_ATTACHMENT_ID_BYTES,
        )?;

        Ok(Self {
            provider,
            ticket_id: ticket_id.to_string(),
            attachment_id: attachment_id.to_string(),
            fetch_url: None,
        })
    }

    pub fn with_fetch_url(mut self, fetch_url: impl Into<String>) -> Self {
        self.fetch_url = Some(fetch_url.into());
        self
    }

    pub fn provider(&self) -> TicketAttachmentProvider {
        self.provider
    }

    pub fn ticket_id(&self) -> &str {
        &self.ticket_id
    }

    pub fn attachment_id(&self) -> &str {
        &self.attachment_id
    }

    pub fn fetch_url(&self) -> Option<&str> {
        self.fetch_url.as_deref()
    }

    pub fn content_pointer(&self) -> Result<TicketAttachmentContentPointer, TicketAttachmentError> {
        TicketAttachmentContentPointer::new(self.provider, &self.ticket_id, &self.attachment_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedTicketAttachmentBytes {
    bytes: Vec<u8>,
}

impl BoundedTicketAttachmentBytes {
    pub fn new(bytes: Vec<u8>) -> Result<Self, TicketAttachmentError> {
        ensure_ticket_attachment_content_size(bytes.len() as u64)?;
        Ok(Self { bytes })
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachmentContentLocation {
    path: PathBuf,
}

impl TicketAttachmentContentLocation {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachmentFetchResult {
    pub descriptor: TicketAttachmentDescriptor,
    pub location: Option<TicketAttachmentContentLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachmentProviderItem {
    pub descriptor: TicketAttachmentDescriptor,
    pub source: TicketAttachmentSourceHandle,
    pub content_fetch_supported: bool,
}

impl TicketAttachmentProviderItem {
    pub fn new(
        descriptor: TicketAttachmentDescriptor,
        source: TicketAttachmentSourceHandle,
        content_fetch_supported: bool,
    ) -> Self {
        Self {
            descriptor,
            source,
            content_fetch_supported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketAttachmentListResult {
    pub attachments: Vec<TicketAttachmentDescriptor>,
    pub(crate) items: Vec<TicketAttachmentProviderItem>,
}

impl TicketAttachmentListResult {
    pub fn new(
        attachments: Vec<TicketAttachmentDescriptor>,
    ) -> Result<Self, TicketAttachmentError> {
        Self::descriptors_only(attachments)
    }

    pub fn from_items(
        items: Vec<TicketAttachmentProviderItem>,
    ) -> Result<Self, TicketAttachmentError> {
        if items.len() > MAX_TICKET_ATTACHMENT_LIST_ITEMS {
            return Err(TicketAttachmentError::TooManyAttachments {
                max: MAX_TICKET_ATTACHMENT_LIST_ITEMS,
            });
        }
        Ok(Self {
            attachments: items.iter().map(|item| item.descriptor.clone()).collect(),
            items,
        })
    }

    pub fn matching_item(
        &self,
        pointer: &TicketAttachmentContentPointer,
    ) -> Option<&TicketAttachmentProviderItem> {
        self.items
            .iter()
            .find(|item| item.descriptor.content_pointer.id() == pointer.id())
    }
}

impl From<TicketAttachmentDescriptor> for TicketAttachmentListResult {
    fn from(descriptor: TicketAttachmentDescriptor) -> Self {
        Self {
            attachments: vec![descriptor],
            items: Vec::new(),
        }
    }
}

impl TicketAttachmentListResult {
    pub fn descriptors_only(
        attachments: Vec<TicketAttachmentDescriptor>,
    ) -> Result<Self, TicketAttachmentError> {
        if attachments.len() > MAX_TICKET_ATTACHMENT_LIST_ITEMS {
            return Err(TicketAttachmentError::TooManyAttachments {
                max: MAX_TICKET_ATTACHMENT_LIST_ITEMS,
            });
        }
        Ok(Self {
            attachments,
            items: Vec::new(),
        })
    }
}

#[async_trait]
pub trait TicketAttachmentProviderReader: Send + Sync {
    async fn list_attachments(
        &self,
        provider: TicketAttachmentProvider,
        ticket_id: &str,
    ) -> Result<TicketAttachmentListResult, TicketAttachmentError>;

    async fn fetch_attachment(
        &self,
        source: &TicketAttachmentSourceHandle,
        max_bytes: usize,
    ) -> Result<BoundedTicketAttachmentBytes, TicketAttachmentError>;
}

#[async_trait]
pub trait TicketAttachmentContentStore: Send + Sync {
    async fn content_location(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError>;

    async fn persist_content(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
        bytes: &BoundedTicketAttachmentBytes,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError>;
}

pub async fn fetch_ticket_attachment_content(
    reader: &dyn TicketAttachmentProviderReader,
    store: &dyn TicketAttachmentContentStore,
    provider: TicketAttachmentProvider,
    ticket_id: &str,
    pointer: &TicketAttachmentContentPointer,
) -> Result<TicketAttachmentFetchResult, TicketAttachmentError> {
    let list_result = reader.list_attachments(provider, ticket_id).await?;
    let item = list_result
        .matching_item(pointer)
        .ok_or(TicketAttachmentError::PointerNotFound)?;

    if !item.content_fetch_supported {
        return Err(TicketAttachmentError::UnsupportedContentFetch);
    }

    if let Some(declared_size_bytes) = item.descriptor.declared_size_bytes {
        ensure_ticket_attachment_content_size(declared_size_bytes)?;
    }

    let bytes = reader
        .fetch_attachment(&item.source, MAX_TICKET_ATTACHMENT_CONTENT_BYTES)
        .await?;
    let bounded = BoundedTicketAttachmentBytes::new(bytes.into_bytes())?;
    let location = store
        .persist_content(&item.source, &item.descriptor.file_name, &bounded)
        .await?;

    Ok(TicketAttachmentFetchResult {
        descriptor: item.descriptor.clone(),
        location: Some(location),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TicketAttachmentError {
    #[error("ticket attachment {field} is required")]
    EmptyField { field: &'static str },
    #[error("ticket attachment {field} exceeds {max} bytes")]
    FieldTooLarge { field: &'static str, max: usize },
    #[error("ticket attachment {field} must not contain path or URL-like input")]
    UnsafeField { field: &'static str },
    #[error("ticket attachment content exceeds {max} bytes")]
    ContentTooLarge { max: usize },
    #[error("ticket attachment list exceeds {max} items")]
    TooManyAttachments { max: usize },
    #[error("ticket attachment provider is unsupported")]
    UnsupportedProvider,
    #[error("ticket attachment content fetch is unsupported")]
    UnsupportedContentFetch,
    #[error("ticket attachment pointer was not found")]
    PointerNotFound,
    #[error("ticket attachment storage path escaped the runtime root")]
    PathEscapedRoot,
    #[error("ticket attachment runtime storage root is unavailable")]
    StorageRootUnavailable,
    #[error("ticket attachment runtime storage write failed")]
    StorageWriteFailed,
    #[error("ticket attachment provider request failed")]
    ProviderRequestFailed,
}

pub fn ensure_ticket_attachment_content_size(size: u64) -> Result<(), TicketAttachmentError> {
    if size > MAX_TICKET_ATTACHMENT_CONTENT_BYTES as u64 {
        return Err(TicketAttachmentError::ContentTooLarge {
            max: MAX_TICKET_ATTACHMENT_CONTENT_BYTES,
        });
    }
    Ok(())
}

pub fn build_ticket_attachment_content_location(
    attachment_root: &Path,
    source: &TicketAttachmentSourceHandle,
    file_name: &str,
) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
    let content_file_name = attachment_content_file_name(file_name)?;
    let provider_dir = fixed_provider_component(source.provider());
    let ticket_dir = hashed_component("ticket", source.ticket_id());
    let attachment_dir = hashed_component("attachment", source.attachment_id());

    let path = attachment_root
        .join(TICKET_ATTACHMENTS_DIR)
        .join(provider_dir)
        .join(ticket_dir)
        .join(attachment_dir)
        .join(content_file_name);

    if !path_has_prefix(&path, attachment_root) {
        return Err(TicketAttachmentError::PathEscapedRoot);
    }

    Ok(TicketAttachmentContentLocation { path })
}

pub fn validate_ticket_attachment_content_parent(
    attachment_root: &Path,
    location: &TicketAttachmentContentLocation,
) -> Result<(), TicketAttachmentError> {
    let canonical_root = attachment_root
        .canonicalize()
        .map_err(|_| TicketAttachmentError::StorageRootUnavailable)?;
    let parent = location
        .path()
        .parent()
        .ok_or(TicketAttachmentError::PathEscapedRoot)?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|_| TicketAttachmentError::StorageRootUnavailable)?;

    if canonical_parent.starts_with(&canonical_root) {
        Ok(())
    } else {
        Err(TicketAttachmentError::PathEscapedRoot)
    }
}

fn validate_identifier(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), TicketAttachmentError> {
    validate_display_value(field, value, max_bytes)?;
    if looks_like_url(value) || has_path_components(value) {
        return Err(TicketAttachmentError::UnsafeField { field });
    }
    Ok(())
}

fn validate_display_value(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), TicketAttachmentError> {
    if value.is_empty() {
        return Err(TicketAttachmentError::EmptyField { field });
    }
    if value.len() > max_bytes {
        return Err(TicketAttachmentError::FieldTooLarge {
            field,
            max: max_bytes,
        });
    }
    Ok(())
}

fn attachment_content_file_name(file_name: &str) -> Result<String, TicketAttachmentError> {
    validate_attachment_file_name(file_name)?;

    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|extension| is_safe_extension(extension));

    Ok(match extension {
        Some(extension) => format!("{CONTENT_FILE_STEM}.{extension}"),
        None => CONTENT_FILE_STEM.to_string(),
    })
}

fn validate_attachment_file_name(file_name: &str) -> Result<(), TicketAttachmentError> {
    validate_display_value(
        "file_name",
        file_name,
        MAX_TICKET_ATTACHMENT_FILE_NAME_BYTES,
    )?;

    if file_name.contains('/') || file_name.contains('\\') {
        return Err(TicketAttachmentError::UnsafeField { field: "file_name" });
    }

    let mut components = Path::new(file_name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(TicketAttachmentError::UnsafeField { field: "file_name" }),
    }
}

fn has_path_components(value: &str) -> bool {
    value.contains('/')
        || value.contains('\\')
        || matches!(
            Path::new(value).components().next(),
            Some(Component::ParentDir | Component::RootDir | Component::Prefix(_))
        )
}

fn looks_like_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("://") || lower.starts_with("bearer ")
}

fn fixed_provider_component(provider: TicketAttachmentProvider) -> &'static str {
    match provider {
        TicketAttachmentProvider::Jira => "provider-jira",
        TicketAttachmentProvider::Linear => "provider-linear",
        TicketAttachmentProvider::ClickUp => "provider-clickup",
    }
}

fn hashed_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{prefix}-{}", hex_prefix(&digest, 12))
}

fn hex_prefix(bytes: &[u8], len: usize) -> String {
    let mut encoded = String::with_capacity(len * 2);
    for byte in &bytes[..len] {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

fn is_safe_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= 16
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn path_has_prefix(path: &Path, expected_prefix: &Path) -> bool {
    let path_components: Vec<_> = path.components().collect();
    let prefix_components: Vec<_> = expected_prefix.components().collect();

    !prefix_components.is_empty()
        && path_components.len() >= prefix_components.len()
        && path_components
            .iter()
            .zip(prefix_components.iter())
            .all(|(path_component, prefix_component)| path_component == prefix_component)
}
