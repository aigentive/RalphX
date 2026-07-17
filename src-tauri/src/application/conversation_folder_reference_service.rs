use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::entities::{
    ChatConversationId, ConversationFolderReference, ConversationFolderReferenceId,
};
use crate::domain::repositories::ConversationFolderReferenceRepository;
use crate::error::{AppError, AppResult};
use crate::utils::path_safety::validate_absolute_non_root_path;

pub const FOLDER_REF_SKIPPED_UNAVAILABLE: &str = "folder_reference_unavailable";
pub const FOLDER_REF_SKIPPED_SYMLINK: &str = "folder_reference_symlink";
pub const FOLDER_REF_SKIPPED_NOT_DIRECTORY: &str = "folder_reference_not_directory";
pub const FOLDER_REF_SKIPPED_CANONICALIZE: &str = "folder_reference_canonicalize_failed";
pub const FOLDER_REF_SKIPPED_CANONICAL_MISMATCH: &str = "folder_reference_canonical_mismatch";
pub const FOLDER_REF_SKIPPED_UNSAFE_PATH: &str = "folder_reference_unsafe_path";
pub const FOLDER_REF_SKIPPED_APP_DATA: &str = "folder_reference_app_data";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderReferenceSkipDiagnostic {
    pub reference_id: ConversationFolderReferenceId,
    pub reason: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValidatedFolderReferences {
    pub references: Vec<ConversationFolderReference>,
    pub skipped: Vec<FolderReferenceSkipDiagnostic>,
}

pub struct ConversationFolderReferenceService {
    repository: Arc<dyn ConversationFolderReferenceRepository>,
    app_data_dir: PathBuf,
    max_live_references: usize,
}

impl ConversationFolderReferenceService {
    pub fn new(
        repository: Arc<dyn ConversationFolderReferenceRepository>,
        app_data_dir: PathBuf,
        max_live_references: usize,
    ) -> Self {
        Self {
            repository,
            app_data_dir,
            max_live_references,
        }
    }

    pub async fn add(
        &self,
        conversation_id: ChatConversationId,
        folder_path: &Path,
        display_name: String,
    ) -> AppResult<ConversationFolderReference> {
        validate_display_name(&display_name)?;
        let canonical_path = validate_registration_path(folder_path, &self.app_data_dir)?;
        let reference = ConversationFolderReference::new(
            conversation_id,
            canonical_path.to_string_lossy(),
            display_name,
        );
        self.repository
            .create_if_below_live_cap(reference, self.max_live_references)
            .await
    }

    pub async fn remove(
        &self,
        id: &ConversationFolderReferenceId,
        conversation_id: &ChatConversationId,
    ) -> AppResult<()> {
        if !self.repository.soft_remove(id, conversation_id).await? {
            return Err(AppError::NotFound(format!(
                "Live folder reference {} was not found for conversation {}",
                id.as_str(),
                conversation_id.as_str()
            )));
        }
        Ok(())
    }

    pub async fn list_live(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Vec<ConversationFolderReference>> {
        self.repository.list_live(conversation_id).await
    }

    pub async fn list_live_validated(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<ValidatedFolderReferences> {
        let references = self.repository.list_live(conversation_id).await?;
        let mut result = ValidatedFolderReferences::default();
        for reference in references {
            match revalidate_stored_path(Path::new(&reference.folder_path), &self.app_data_dir) {
                Ok(_) => result.references.push(reference),
                Err(reason) => {
                    tracing::warn!(
                        conversation_id = conversation_id.as_str(),
                        folder_reference_id = reference.id.as_str(),
                        reason,
                        "folder_refs_skipped"
                    );
                    result.skipped.push(FolderReferenceSkipDiagnostic {
                        reference_id: reference.id,
                        reason,
                    });
                }
            }
        }
        Ok(result)
    }

    pub async fn render_prompt_block(
        &self,
        conversation_id: &ChatConversationId,
    ) -> AppResult<Option<String>> {
        let references = self.list_live_validated(conversation_id).await?.references;
        Ok(render_prompt_block(&references))
    }

    pub async fn render_prompt_block_for_roots(
        &self,
        conversation_id: &ChatConversationId,
        roots: &[PathBuf],
    ) -> AppResult<Option<String>> {
        let references = self
            .list_live_validated(conversation_id)
            .await?
            .references
            .into_iter()
            .filter(|reference| roots.contains(&PathBuf::from(&reference.folder_path)))
            .collect::<Vec<_>>();
        Ok(render_prompt_block(&references))
    }
}

fn render_prompt_block(references: &[ConversationFolderReference]) -> Option<String> {
    if references.is_empty() {
        return None;
    }
    let entries = references
        .iter()
        .map(|reference| {
            format!(
                "  <folder path=\"{}\" display_name=\"{}\" />",
                escape_xml(&reference.folder_path),
                escape_xml(&reference.display_name)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "<referenced_folders>\n{entries}\n</referenced_folders>"
    ))
}

fn validate_display_name(display_name: &str) -> AppResult<()> {
    if display_name.chars().any(char::is_control) {
        return Err(AppError::Validation(
            "Folder display name must not contain control characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_registration_path(path: &Path, app_data_dir: &Path) -> AppResult<PathBuf> {
    validate_absolute_non_root_path(path, "conversation folder reference")?;
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        AppError::Validation(format!("Folder reference is unavailable: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Validation(
            "Folder reference root must not be a symlink".to_string(),
        ));
    }
    if !metadata.is_dir() {
        return Err(AppError::Validation(
            "Folder reference must point to an existing directory".to_string(),
        ));
    }
    let canonical = fs::canonicalize(path).map_err(|error| {
        AppError::Validation(format!("Folder reference cannot be canonicalized: {error}"))
    })?;
    let canonical = validate_absolute_non_root_path(&canonical, "conversation folder reference")?;
    reject_app_data_path(&canonical, app_data_dir)?;
    Ok(canonical)
}

fn reject_app_data_path(path: &Path, app_data_dir: &Path) -> AppResult<()> {
    let app_data_dir = fs::canonicalize(app_data_dir).map_err(|error| {
        AppError::ConversationFolderReferenceAppDataUnavailable {
            detail: error.to_string(),
        }
    })?;
    let app_data_dir = validate_absolute_non_root_path(&app_data_dir, "application data root")?;
    if path.starts_with(&app_data_dir) || app_data_dir.starts_with(path) {
        return Err(AppError::Validation(
            "Folder reference must not contain or be inside RalphX application data".to_string(),
        ));
    }
    Ok(())
}

pub fn revalidate_stored_path(path: &Path, app_data_dir: &Path) -> Result<PathBuf, &'static str> {
    validate_absolute_non_root_path(path, "conversation folder reference")
        .map_err(|_| FOLDER_REF_SKIPPED_UNSAFE_PATH)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| FOLDER_REF_SKIPPED_UNAVAILABLE)?;
    if metadata.file_type().is_symlink() {
        return Err(FOLDER_REF_SKIPPED_SYMLINK);
    }
    if !metadata.is_dir() {
        return Err(FOLDER_REF_SKIPPED_NOT_DIRECTORY);
    }
    let canonical = fs::canonicalize(path).map_err(|_| FOLDER_REF_SKIPPED_CANONICALIZE)?;
    let canonical = validate_absolute_non_root_path(&canonical, "conversation folder reference")
        .map_err(|_| FOLDER_REF_SKIPPED_UNSAFE_PATH)?;
    reject_app_data_path(&canonical, app_data_dir).map_err(|_| FOLDER_REF_SKIPPED_APP_DATA)?;
    if canonical != path {
        return Err(FOLDER_REF_SKIPPED_CANONICAL_MISMATCH);
    }
    Ok(canonical)
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
