use std::collections::VecDeque;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

pub const MAX_INGEST_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_INGEST_TOTAL_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_INGEST_FILES: u64 = 500;
pub const MAX_INGEST_DEPTH: usize = 12;

const PERSONA_INGEST_DIR: &str = "persona_ingest";
const CONTENT_FILE_NAME: &str = "content";
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaIngestEntry {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaIngestManifest {
    pub copied: Vec<PersonaIngestEntry>,
    pub skipped: Vec<PersonaIngestEntry>,
    pub rejected: Vec<PersonaIngestEntry>,
}

/// Returns the app-owned root used for copied persona source material.
pub fn persona_ingest_storage_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PERSONA_INGEST_DIR)
}

/// Returns a hash-addressed conversation root beneath the app-owned ingest root.
pub fn persona_ingest_conversation_path(storage_root: &Path, conversation_id: &str) -> PathBuf {
    storage_root.join(hashed_component("conversation", conversation_id))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PersonaBuilderIngestSessionLiveness {
    MissingAppDataDirectory,
    InvalidRoot,
    MissingRoot,
    UnreadableRoot,
    EmptyRoot,
}

/// Returns the validated non-empty ingest root for a PersonaBuilder conversation.
pub(crate) fn live_persona_builder_ingest_root(
    app_data_dir: Option<&Path>,
    conversation_id: &str,
) -> Result<PathBuf, PersonaBuilderIngestSessionLiveness> {
    let app_data_dir =
        app_data_dir.ok_or(PersonaBuilderIngestSessionLiveness::MissingAppDataDirectory)?;
    let ingest_root = persona_ingest_conversation_path(
        &persona_ingest_storage_path(app_data_dir),
        conversation_id,
    );
    let ingest_root = crate::utils::path_safety::validate_absolute_non_root_path(
        &ingest_root,
        "PersonaBuilder MCP filesystem read root",
    )
    .map_err(|_| PersonaBuilderIngestSessionLiveness::InvalidRoot)?;
    if !ingest_root.is_dir() {
        return Err(PersonaBuilderIngestSessionLiveness::MissingRoot);
    }
    // codeql[rust/path-injection]
    let mut entries = fs::read_dir(&ingest_root)
        .map_err(|_| PersonaBuilderIngestSessionLiveness::UnreadableRoot)?;
    if entries.next().is_none() {
        return Err(PersonaBuilderIngestSessionLiveness::EmptyRoot);
    }

    Ok(ingest_root)
}

/// True when the conversation's ingest store exists, is validated, and is non-empty —
/// the spec's definition of a live PersonaBuilder draft ingest session (A11).
pub fn persona_builder_ingest_session_is_live(
    app_data_dir: Option<&Path>,
    conversation_id: &str,
) -> bool {
    live_persona_builder_ingest_root(app_data_dir, conversation_id).is_ok()
}

/// Builds a hash-addressed destination for one validated relative source path.
///
/// # Errors
/// Returns an error when `relative_path` is empty, rooted, or traverses parents.
pub fn build_persona_ingest_file_path(
    destination_root: &Path,
    relative_path: &Path,
) -> AppResult<PathBuf> {
    let relative_display = relative_display_path(relative_path)?;
    Ok(destination_root
        .join(hashed_component("file", &relative_display))
        .join(CONTENT_FILE_NAME))
}

/// Copies safe text source files into a pre-resolved app-owned destination root.
///
/// # Errors
/// Returns an error when the picked root or app-owned destination cannot be safely accessed.
pub fn ingest_picked_root(
    picked_root: &Path,
    destination_root: &Path,
) -> AppResult<PersonaIngestManifest> {
    let canonical_root = picked_root
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize the picked path", error))?;
    let root_metadata = fs::symlink_metadata(picked_root)
        .map_err(|error| filesystem_error("inspect the picked path", error))?;
    if root_metadata.file_type().is_symlink() {
        return Err(AppError::Validation(
            "Picked path must not be a symlink".to_string(),
        ));
    }

    let canonical_destination_root = ensure_destination_root(destination_root)?;
    let mut manifest = PersonaIngestManifest::default();
    let mut usage = IngestUsage::default();

    if root_metadata.is_file() {
        let file_name = picked_root
            .file_name()
            .ok_or_else(|| AppError::Validation("Picked file must have a file name".to_string()))?;
        let relative_path = safe_relative_join(Path::new(""), Path::new(file_name))?;
        ingest_file(
            &canonical_root,
            &canonical_root,
            &relative_path,
            0,
            &canonical_destination_root,
            &mut usage,
            &mut manifest,
        )?;
    } else if root_metadata.is_dir() {
        ingest_directory_tree(
            &canonical_root,
            &canonical_destination_root,
            &mut usage,
            &mut manifest,
        )?;
    } else {
        return Err(AppError::Validation(
            "Picked path must be a regular file or directory".to_string(),
        ));
    }

    persist_manifest(&canonical_destination_root, &manifest)?;
    Ok(manifest)
}

#[derive(Default)]
struct IngestUsage {
    files: u64,
    bytes: u64,
}

fn ingest_directory_tree(
    canonical_root: &Path,
    destination_root: &Path,
    usage: &mut IngestUsage,
    manifest: &mut PersonaIngestManifest,
) -> AppResult<()> {
    let mut directories = VecDeque::from([(canonical_root.to_path_buf(), PathBuf::new(), 0usize)]);

    while let Some((directory, relative_directory, depth)) = directories.pop_front() {
        require_under_root(&directory, canonical_root, "directory traversal")?;
        // codeql[rust/path-injection]
        let entries = fs::read_dir(&directory)
            .map_err(|error| filesystem_error("read a picked directory", error))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| filesystem_error("read a picked directory entry", error))?;
            let file_name = entry.file_name();
            let relative_path = safe_relative_join(&relative_directory, Path::new(&file_name))?;
            let display_path = relative_display_path(&relative_path)?;
            let source_path = safe_child_path(&directory, Path::new(&file_name), canonical_root)?;
            // codeql[rust/path-injection]
            let metadata = fs::symlink_metadata(&source_path)
                .map_err(|error| filesystem_error("inspect a picked directory entry", error))?;

            if metadata.file_type().is_symlink() {
                manifest
                    .rejected
                    .push(rejected_entry(display_path, "symlinks are not accepted"));
                continue;
            }

            let child_depth = depth + 1;
            if child_depth > MAX_INGEST_DEPTH {
                manifest
                    .skipped
                    .push(skipped_entry(display_path, "maximum ingest depth exceeded"));
                continue;
            }

            if metadata.is_dir() {
                directories.push_back((source_path, relative_path, child_depth));
            } else if metadata.is_file() {
                ingest_file(
                    canonical_root,
                    &source_path,
                    &relative_path,
                    child_depth,
                    destination_root,
                    usage,
                    manifest,
                )?;
            } else {
                manifest
                    .rejected
                    .push(rejected_entry(display_path, "unsupported filesystem entry"));
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn ingest_file(
    canonical_root: &Path,
    source_path: &Path,
    relative_path: &Path,
    depth: usize,
    destination_root: &Path,
    usage: &mut IngestUsage,
    manifest: &mut PersonaIngestManifest,
) -> AppResult<()> {
    let display_path = relative_display_path(relative_path)?;
    if depth > MAX_INGEST_DEPTH {
        manifest
            .skipped
            .push(skipped_entry(display_path, "maximum ingest depth exceeded"));
        return Ok(());
    }
    if !is_allowed_text_path(relative_path) {
        manifest
            .skipped
            .push(skipped_entry(display_path, "unsupported file type"));
        return Ok(());
    }
    // codeql[rust/path-injection]
    let metadata = fs::symlink_metadata(source_path)
        .map_err(|error| filesystem_error("inspect a picked file", error))?;
    if metadata.file_type().is_symlink() {
        manifest
            .rejected
            .push(rejected_entry(display_path, "symlinks are not accepted"));
        return Ok(());
    }
    if metadata.len() > MAX_INGEST_FILE_BYTES {
        manifest.skipped.push(skipped_entry(
            display_path,
            "file size exceeds ingest limit",
        ));
        return Ok(());
    }
    if usage.files >= MAX_INGEST_FILES {
        manifest.skipped.push(skipped_entry(
            display_path,
            "file count exceeds ingest limit",
        ));
        return Ok(());
    }
    if usage.bytes.saturating_add(metadata.len()) > MAX_INGEST_TOTAL_BYTES {
        manifest
            .skipped
            .push(skipped_entry(display_path, "total byte limit exceeded"));
        return Ok(());
    }

    // codeql[rust/path-injection]
    let canonical_file = source_path
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize a picked file", error))?;
    if !canonical_file.starts_with(canonical_root) {
        manifest.rejected.push(rejected_entry(
            display_path,
            "file resolves outside the picked path",
        ));
        return Ok(());
    }
    // codeql[rust/path-injection]
    let canonical_metadata = fs::metadata(&canonical_file)
        .map_err(|error| filesystem_error("inspect a canonical picked file", error))?;
    if !canonical_metadata.is_file() || canonical_metadata.len() > MAX_INGEST_FILE_BYTES {
        manifest
            .skipped
            .push(skipped_entry(display_path, "file changed before ingest"));
        return Ok(());
    }
    if usage.bytes.saturating_add(canonical_metadata.len()) > MAX_INGEST_TOTAL_BYTES {
        manifest
            .skipped
            .push(skipped_entry(display_path, "total byte limit exceeded"));
        return Ok(());
    }

    let mut contents = Vec::with_capacity(canonical_metadata.len() as usize);
    // codeql[rust/path-injection]
    fs::File::open(&canonical_file)
        .map_err(|error| filesystem_error("open a canonical picked file", error))?
        .read_to_end(&mut contents)
        .map_err(|error| filesystem_error("read a canonical picked file", error))?;
    if std::str::from_utf8(&contents).is_err() {
        manifest
            .skipped
            .push(skipped_entry(display_path, "file is not valid UTF-8 text"));
        return Ok(());
    }

    let destination = build_persona_ingest_file_path(destination_root, relative_path)?;
    let safe_destination = prepare_destination_file(destination_root, &destination)?;
    // codeql[rust/path-injection]
    fs::write(&safe_destination, &contents)
        .map_err(|error| filesystem_error("write an app-owned ingest copy", error))?;
    usage.files += 1;
    usage.bytes += contents.len() as u64;
    manifest.copied.push(PersonaIngestEntry {
        path: display_path,
        reason: None,
    });
    Ok(())
}

fn ensure_destination_root(destination_root: &Path) -> AppResult<PathBuf> {
    // codeql[rust/path-injection]
    fs::create_dir_all(destination_root)
        .map_err(|error| filesystem_error("create the app-owned ingest root", error))?;
    // codeql[rust/path-injection]
    destination_root
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize the app-owned ingest root", error))
}

fn prepare_destination_file(destination_root: &Path, destination: &Path) -> AppResult<PathBuf> {
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Validation("Ingest destination must have a parent".to_string()))?;
    require_under_root(parent, destination_root, "ingest destination")?;
    // codeql[rust/path-injection]
    fs::create_dir_all(parent)
        .map_err(|error| filesystem_error("create an app-owned ingest directory", error))?;
    // codeql[rust/path-injection]
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize an app-owned ingest directory", error))?;
    require_under_root(&canonical_parent, destination_root, "ingest destination")?;
    let safe_destination = canonical_parent.join(CONTENT_FILE_NAME);
    require_under_root(&safe_destination, destination_root, "ingest destination")?;
    // codeql[rust/path-injection]
    if fs::symlink_metadata(&safe_destination)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(
            "App-owned ingest destination must not be a symlink".to_string(),
        ));
    }
    Ok(safe_destination)
}

fn persist_manifest(destination_root: &Path, manifest: &PersonaIngestManifest) -> AppResult<()> {
    let manifest_path = destination_root.join(MANIFEST_FILE_NAME);
    require_under_root(&manifest_path, destination_root, "ingest manifest")?;
    // codeql[rust/path-injection]
    if fs::symlink_metadata(&manifest_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(
            "App-owned ingest manifest must not be a symlink".to_string(),
        ));
    }
    let encoded = serde_json::to_vec_pretty(manifest).map_err(|error| {
        AppError::Infrastructure(format!("Failed to serialize ingest manifest: {error}"))
    })?;
    // codeql[rust/path-injection]
    fs::write(manifest_path, encoded)
        .map_err(|error| filesystem_error("write the app-owned ingest manifest", error))
}

fn safe_relative_join(parent: &Path, child: &Path) -> AppResult<PathBuf> {
    validate_single_component(child)?;
    let candidate = parent.join(child);
    relative_display_path(&candidate)?;
    Ok(candidate)
}

fn safe_child_path(parent: &Path, child: &Path, canonical_root: &Path) -> AppResult<PathBuf> {
    validate_single_component(child)?;
    let candidate = parent.join(child);
    require_under_root(&candidate, canonical_root, "picked path")?;
    Ok(candidate)
}

fn relative_display_path(path: &Path) -> AppResult<String> {
    if path.as_os_str().is_empty() {
        return Err(AppError::Validation(
            "Relative path must not be empty".to_string(),
        ));
    }
    let mut components = Vec::new();
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(AppError::Validation(
                "Relative path contains unsafe components".to_string(),
            ));
        };
        let rendered = name.to_string_lossy();
        if rendered.is_empty() || rendered.contains('/') || rendered.contains('\\') {
            return Err(AppError::Validation(
                "Relative path contains unsafe components".to_string(),
            ));
        }
        components.push(rendered.into_owned());
    }
    if components.is_empty() {
        return Err(AppError::Validation(
            "Relative path must not be empty".to_string(),
        ));
    }
    Ok(components.join("/"))
}

fn validate_single_component(path: &Path) -> AppResult<()> {
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(name)), None)
            if !name.is_empty()
                && !name.to_string_lossy().contains('/')
                && !name.to_string_lossy().contains('\\') =>
        {
            Ok(())
        }
        _ => Err(AppError::Validation(
            "Path component must be a single normal component".to_string(),
        )),
    }
}

fn require_under_root(path: &Path, root: &Path, label: &str) -> AppResult<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "{label} escapes its allowed root"
        )))
    }
}

fn is_allowed_text_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some(extension) if matches!(extension.to_ascii_lowercase().as_str(),
            "txt" | "md" | "mdx" | "rst" | "adoc" | "json" | "yaml" | "yml" | "toml"
            | "ini" | "cfg" | "conf" | "log" | "rs" | "ts" | "tsx" | "js" | "jsx"
            | "py" | "go" | "java" | "kt" | "swift" | "c" | "h" | "cpp" | "hpp"
            | "cs" | "rb" | "php" | "html" | "css" | "scss" | "sql" | "sh" | "bash"
            | "zsh" | "fish" | "xml" | "csv"
        )
    )
}

fn skipped_entry(path: String, reason: &str) -> PersonaIngestEntry {
    PersonaIngestEntry {
        path,
        reason: Some(reason.to_string()),
    }
}

fn rejected_entry(path: String, reason: &str) -> PersonaIngestEntry {
    PersonaIngestEntry {
        path,
        reason: Some(reason.to_string()),
    }
}

fn hashed_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let encoded = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{encoded}")
}

fn filesystem_error(action: &str, error: std::io::Error) -> AppError {
    AppError::Infrastructure(format!("Failed to {action}: {error}"))
}
