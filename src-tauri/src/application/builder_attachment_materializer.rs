use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::application::chat_attachment_storage::build_builder_workspace_attachment_path;
use crate::application::standalone_workspace::{create_workspace, resolve_workspace};
use crate::domain::entities::{ChatAttachment, ChatConversationId};
use crate::domain::repositories::ChatAttachmentRepository;
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::{filesystem_error, require_under_root};

pub fn validate_builder_attachment_text(bytes: &[u8]) -> AppResult<()> {
    if bytes.contains(&0) || std::str::from_utf8(bytes).is_err() {
        return Err(AppError::PersonaBuilderTextAttachmentOnly);
    }
    Ok(())
}

/// Resolves the deterministic workspace path for an already materialized attachment.
///
/// # Errors
/// Returns a typed error when the conversation workspace is missing or the normalized
/// attachment destination would escape it.
pub fn materialized_builder_attachment_path(
    app_data_dir: &Path,
    attachment: &ChatAttachment,
) -> AppResult<PathBuf> {
    let workspace = resolve_workspace(app_data_dir, &attachment.conversation_id.as_str())?;
    let path =
        build_builder_workspace_attachment_path(&workspace, &attachment.id, &attachment.file_name)
            .map_err(AppError::Validation)?;
    require_under_root(&path, &workspace, "builder workspace attachment")?;
    // codeql[rust/path-injection]
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| filesystem_error("inspect a materialized builder attachment", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Validation(
            "Materialized builder attachment must be a non-symlink file".to_string(),
        ));
    }
    // codeql[rust/path-injection]
    let canonical_path = path.canonicalize().map_err(|error| {
        filesystem_error("canonicalize a materialized builder attachment", error)
    })?;
    require_under_root(
        &canonical_path,
        &workspace,
        "materialized builder attachment",
    )?;
    Ok(canonical_path)
}

/// Idempotently removes one materialized builder attachment from its contained workspace.
///
/// # Errors
/// Returns a typed error when an existing workspace destination is unsafe or cannot be removed.
pub fn remove_materialized_builder_attachment_if_present(
    app_data_dir: &Path,
    attachment: &ChatAttachment,
) -> AppResult<()> {
    let workspace = match resolve_workspace(app_data_dir, &attachment.conversation_id.as_str()) {
        Ok(workspace) => workspace,
        Err(AppError::StandaloneWorkspaceMissing { .. }) => return Ok(()),
        Err(error) => return Err(error),
    };
    let path =
        build_builder_workspace_attachment_path(&workspace, &attachment.id, &attachment.file_name)
            .map_err(AppError::Validation)?;
    require_under_root(&path, &workspace, "builder workspace attachment")?;
    // codeql[rust/path-injection]
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(filesystem_error(
                "inspect a materialized builder attachment for deletion",
                error,
            ))
        }
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Validation(
            "Materialized builder attachment must be a non-symlink file".to_string(),
        ));
    }
    // codeql[rust/path-injection]
    let canonical_path = path.canonicalize().map_err(|error| {
        filesystem_error(
            "canonicalize a materialized builder attachment for deletion",
            error,
        )
    })?;
    require_under_root(
        &canonical_path,
        &workspace,
        "materialized builder attachment",
    )?;
    // codeql[rust/path-injection]
    fs::remove_file(&canonical_path)
        .map_err(|error| filesystem_error("remove a materialized builder attachment", error))
}

/// Copies one text attachment from app-owned storage into its conversation workspace.
///
/// # Errors
/// Returns a typed error for binary content, unsafe/symlinked source or destination
/// paths, missing storage, or filesystem failures.
pub fn materialize_builder_attachment(
    app_data_dir: &Path,
    attachment_storage_root: &Path,
    attachment: &ChatAttachment,
) -> AppResult<PathBuf> {
    let workspace = create_workspace(app_data_dir, &attachment.conversation_id.as_str())?;
    let source = canonical_attachment_source(attachment_storage_root, attachment)?;
    // codeql[rust/path-injection]
    let bytes = fs::read(&source)
        .map_err(|error| filesystem_error("read a builder attachment source", error))?;
    validate_builder_attachment_text(&bytes)?;

    let destination =
        build_builder_workspace_attachment_path(&workspace, &attachment.id, &attachment.file_name)
            .map_err(AppError::Validation)?;
    require_under_root(&destination, &workspace, "builder workspace attachment")?;
    let parent = destination.parent().ok_or_else(|| {
        AppError::Validation("Builder workspace attachment has no parent directory".to_string())
    })?;
    require_under_root(parent, &workspace, "builder workspace attachment directory")?;
    let attachments_root = parent.parent().ok_or_else(|| {
        AppError::Validation(
            "Builder workspace attachment directory has no contained root".to_string(),
        )
    })?;
    require_under_root(
        attachments_root,
        &workspace,
        "builder workspace attachments root",
    )?;
    reject_symlink(attachments_root, "Builder workspace attachments root")?;
    // codeql[rust/path-injection]
    match fs::create_dir(attachments_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(filesystem_error(
                "create the builder workspace attachments root",
                error,
            ))
        }
    }
    // codeql[rust/path-injection]
    let canonical_attachments_root = attachments_root.canonicalize().map_err(|error| {
        filesystem_error("canonicalize the builder workspace attachments root", error)
    })?;
    require_under_root(
        &canonical_attachments_root,
        &workspace,
        "builder workspace attachments root",
    )?;
    reject_symlink(parent, "Builder workspace attachment directory")?;
    // codeql[rust/path-injection]
    match fs::create_dir(parent) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(filesystem_error(
                "create a builder workspace attachment directory",
                error,
            ))
        }
    }
    // codeql[rust/path-injection]
    let canonical_parent = parent.canonicalize().map_err(|error| {
        filesystem_error(
            "canonicalize a builder workspace attachment directory",
            error,
        )
    })?;
    require_under_root(
        &canonical_parent,
        &canonical_attachments_root,
        "builder workspace attachment directory",
    )?;
    if fs::symlink_metadata(&destination)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(
            "Builder workspace attachment path must not be a symlink".to_string(),
        ));
    }
    // Revalidate the final sink after parent creation; no DB-derived component reaches
    // the write without the hash-derived id and normalized content leaf builder above.
    require_under_root(&destination, &workspace, "builder workspace attachment")?;
    // codeql[rust/path-injection]
    fs::write(&destination, bytes)
        .map_err(|error| filesystem_error("write a builder workspace attachment", error))?;
    Ok(destination)
}

/// Idempotently materializes every stored attachment for one builder conversation.
///
/// # Errors
/// Returns the first repository, validation, containment, or filesystem error and does
/// not report a successful sync when any attachment is unavailable.
pub async fn sync_builder_attachments(
    app_data_dir: &Path,
    attachment_storage_root: &Path,
    conversation_id: &ChatConversationId,
    repository: Arc<dyn ChatAttachmentRepository>,
) -> AppResult<Vec<PathBuf>> {
    create_workspace(app_data_dir, &conversation_id.as_str())?;
    let attachments = repository.find_by_conversation_id(conversation_id).await?;
    let mut paths = Vec::with_capacity(attachments.len());
    for attachment in attachments {
        paths.push(materialize_builder_attachment(
            app_data_dir,
            attachment_storage_root,
            &attachment,
        )?);
    }
    Ok(paths)
}

fn reject_symlink(path: &Path, label: &str) -> AppResult<()> {
    if fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(format!(
            "{label} must not be a symlink"
        )));
    }
    Ok(())
}

fn canonical_attachment_source(
    attachment_storage_root: &Path,
    attachment: &ChatAttachment,
) -> AppResult<PathBuf> {
    // codeql[rust/path-injection]
    let canonical_root = attachment_storage_root.canonicalize().map_err(|error| {
        filesystem_error("canonicalize the chat attachment storage root", error)
    })?;
    let source = PathBuf::from(&attachment.file_path);
    // codeql[rust/path-injection]
    let metadata = fs::symlink_metadata(&source)
        .map_err(|error| filesystem_error("inspect a builder attachment source", error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Validation(
            "Builder attachment source must be a non-symlink file".to_string(),
        ));
    }
    // codeql[rust/path-injection]
    let canonical_source = source
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize a builder attachment source", error))?;
    require_under_root(
        &canonical_source,
        &canonical_root,
        "builder attachment source",
    )?;
    Ok(canonical_source)
}
