use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;

use crate::application::app_paths::AppPaths;
use crate::application::ticket_attachment::{
    build_ticket_attachment_content_location, validate_ticket_attachment_content_parent,
    BoundedTicketAttachmentBytes, TicketAttachmentContentLocation, TicketAttachmentContentStore,
    TicketAttachmentError, TicketAttachmentSourceHandle,
};

#[derive(Debug, Clone)]
pub struct TicketAttachmentRuntimeStore {
    attachment_root: PathBuf,
}

impl TicketAttachmentRuntimeStore {
    pub fn new(attachment_root: impl Into<PathBuf>) -> Self {
        Self {
            attachment_root: attachment_root.into(),
        }
    }

    pub fn from_app_paths(app_paths: &AppPaths) -> Self {
        Self::new(app_paths.attachment_storage_path())
    }

    pub fn attachment_root(&self) -> &Path {
        &self.attachment_root
    }

    fn build_location(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
        build_ticket_attachment_content_location(&self.attachment_root, source, file_name)
    }
}

#[async_trait]
impl TicketAttachmentContentStore for TicketAttachmentRuntimeStore {
    async fn content_location(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
        self.build_location(source, file_name)
    }

    async fn persist_content(
        &self,
        source: &TicketAttachmentSourceHandle,
        file_name: &str,
        bytes: &BoundedTicketAttachmentBytes,
    ) -> Result<TicketAttachmentContentLocation, TicketAttachmentError> {
        let location = self.build_location(source, file_name)?;
        tokio::fs::create_dir_all(&self.attachment_root)
            .await
            .map_err(|_| TicketAttachmentError::StorageRootUnavailable)?;

        let parent = location
            .path()
            .parent()
            .ok_or(TicketAttachmentError::PathEscapedRoot)?;
        ensure_runtime_parent(&self.attachment_root, parent).await?;
        validate_ticket_attachment_content_parent(&self.attachment_root, &location)?;

        let temp_path = parent.join(format!(".content-{}.tmp", stable_temp_suffix()));
        if let Err(error) = write_atomic(&self.attachment_root, &location, &temp_path, bytes).await
        {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(error);
        }

        Ok(location)
    }
}

async fn ensure_runtime_parent(
    attachment_root: &Path,
    parent: &Path,
) -> Result<(), TicketAttachmentError> {
    let relative_parent = parent
        .strip_prefix(attachment_root)
        .map_err(|_| TicketAttachmentError::PathEscapedRoot)?;
    let mut current = attachment_root.to_path_buf();

    for component in relative_parent.components() {
        let Component::Normal(name) = component else {
            return Err(TicketAttachmentError::PathEscapedRoot);
        };
        current.push(name);
        ensure_single_runtime_dir(&current).await?;
    }

    Ok(())
}

async fn ensure_single_runtime_dir(path: &Path) -> Result<(), TicketAttachmentError> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(TicketAttachmentError::PathEscapedRoot);
            }
            if metadata.is_dir() {
                return Ok(());
            }
            Err(TicketAttachmentError::StorageRootUnavailable)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            match tokio::fs::create_dir(path).await {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    let metadata = tokio::fs::symlink_metadata(path)
                        .await
                        .map_err(|_| TicketAttachmentError::StorageRootUnavailable)?;
                    if metadata.file_type().is_symlink() {
                        return Err(TicketAttachmentError::PathEscapedRoot);
                    }
                    if metadata.is_dir() {
                        Ok(())
                    } else {
                        Err(TicketAttachmentError::StorageRootUnavailable)
                    }
                }
                Err(_) => Err(TicketAttachmentError::StorageRootUnavailable),
            }
        }
        Err(_) => Err(TicketAttachmentError::StorageRootUnavailable),
    }
}

async fn write_atomic(
    attachment_root: &Path,
    location: &TicketAttachmentContentLocation,
    temp_path: &Path,
    bytes: &BoundedTicketAttachmentBytes,
) -> Result<(), TicketAttachmentError> {
    tokio::fs::write(temp_path, bytes.as_slice())
        .await
        .map_err(|_| TicketAttachmentError::StorageWriteFailed)?;

    validate_ticket_attachment_content_parent(attachment_root, location)?;

    tokio::fs::rename(temp_path, location.path())
        .await
        .map_err(|_| TicketAttachmentError::StorageWriteFailed)?;

    validate_ticket_attachment_content_parent(attachment_root, location)
}

fn stable_temp_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}
