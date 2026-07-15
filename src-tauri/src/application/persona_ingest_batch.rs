use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::application::persona_ingest::{
    ensure_destination_root, filesystem_error, ingest_validated_picked_root, require_under_root,
    IngestUsage, PersonaIngestEntry, PersonaIngestManifest, CONTENT_FILE_NAME, MANIFEST_FILE_NAME,
};
use crate::error::{AppError, AppResult};

static PERSONA_INGEST_LOCK: Mutex<()> = Mutex::new(());

struct PreparedPickedRoot {
    picked_path: PathBuf,
    canonical_root: PathBuf,
    metadata: fs::Metadata,
}

/// Ingests a batch into one app-owned destination and persists a cumulative manifest.
///
/// The returned manifest contains only this batch. The persisted manifest merges all
/// successful batches for the destination.
///
/// # Errors
/// Returns an error for an empty/all-invalid batch or unsafe app-owned storage state.
pub fn ingest_picked_roots(
    picked_roots: &[PathBuf],
    destination_root: &Path,
) -> AppResult<PersonaIngestManifest> {
    if picked_roots.is_empty() {
        return Err(AppError::Validation(
            "Persona context requires at least one picked path".to_string(),
        ));
    }

    let mut batch_manifest = PersonaIngestManifest::default();
    let mut prepared = Vec::new();
    for picked_root in picked_roots {
        match prepare_picked_root(picked_root) {
            Ok(root) => prepared.push(root),
            Err(reason) => batch_manifest.rejected.push(PersonaIngestEntry {
                path: picked_basename(picked_root),
                reason: Some(reason.to_string()),
            }),
        }
    }
    if prepared.is_empty() {
        let reason = batch_manifest
            .rejected
            .first()
            .and_then(|entry| entry.reason.as_deref())
            .unwrap_or("picked paths are unavailable");
        return Err(AppError::Validation(format!(
            "No persona context paths could be ingested: {reason}"
        )));
    }

    let _guard = PERSONA_INGEST_LOCK.lock().map_err(|_| {
        AppError::Infrastructure("Persona context ingest lock is unavailable".to_string())
    })?;
    let canonical_destination_root = ensure_destination_root(destination_root)?;
    let mut cumulative_manifest = load_manifest(&canonical_destination_root)?;
    let mut usage = seed_usage(&canonical_destination_root)?;

    for root in prepared {
        ingest_validated_picked_root(
            &root.picked_path,
            &root.canonical_root,
            &root.metadata,
            &canonical_destination_root,
            &mut usage,
            &mut batch_manifest,
        )?;
    }

    merge_manifest(&mut cumulative_manifest, &batch_manifest);
    persist_manifest(&canonical_destination_root, &cumulative_manifest)?;
    Ok(batch_manifest)
}

fn prepare_picked_root(picked_root: &Path) -> Result<PreparedPickedRoot, &'static str> {
    let picked_path = crate::utils::path_safety::validate_absolute_non_root_path(
        picked_root,
        "Persona context picked path",
    )
    .map_err(|_| "picked path must be absolute and traversal-free")?;
    // codeql[rust/path-injection]
    let metadata = fs::symlink_metadata(&picked_path)
        .map_err(|_| "picked path is unavailable")?;
    if metadata.file_type().is_symlink() {
        return Err("picked path must not be a symlink");
    }
    if !metadata.is_file() && !metadata.is_dir() {
        return Err("picked path must be a regular file or directory");
    }
    // codeql[rust/path-injection]
    let canonical_root = picked_path
        .canonicalize()
        .map_err(|_| "picked path is unavailable")?;
    Ok(PreparedPickedRoot {
        picked_path,
        canonical_root,
        metadata,
    })
}

fn picked_basename(path: &Path) -> String {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.contains('/') && !name.contains('\\'))
        .unwrap_or_else(|| "picked path".to_string())
}

fn seed_usage(destination_root: &Path) -> AppResult<IngestUsage> {
    let mut usage = IngestUsage::default();
    // codeql[rust/path-injection]
    let entries = fs::read_dir(destination_root)
        .map_err(|error| filesystem_error("scan app-owned ingest usage", error))?;
    for entry in entries {
        let entry = entry.map_err(|error| filesystem_error("scan ingest entry", error))?;
        let entry_path = entry.path();
        require_under_root(&entry_path, destination_root, "ingest usage entry")?;
        // codeql[rust/path-injection]
        let metadata = fs::symlink_metadata(&entry_path)
            .map_err(|error| filesystem_error("inspect ingest usage entry", error))?;
        if metadata.file_type().is_symlink() {
            return Err(AppError::Validation(
                "App-owned ingest usage entries must not be symlinks".to_string(),
            ));
        }
        if !metadata.is_dir() || !is_hashed_file_directory(&entry.file_name().to_string_lossy()) {
            continue;
        }
        // codeql[rust/path-injection]
        let canonical_entry = entry_path
            .canonicalize()
            .map_err(|error| filesystem_error("canonicalize ingest usage entry", error))?;
        require_under_root(&canonical_entry, destination_root, "ingest usage entry")?;
        let content_path = canonical_entry.join(CONTENT_FILE_NAME);
        require_under_root(&content_path, destination_root, "ingest usage content")?;
        // codeql[rust/path-injection]
        let content_metadata = match fs::symlink_metadata(&content_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(filesystem_error("inspect ingest usage content", error)),
        };
        if content_metadata.file_type().is_symlink() || !content_metadata.is_file() {
            return Err(AppError::Validation(
                "App-owned ingest content must be a regular file".to_string(),
            ));
        }
        usage.files = usage.files.saturating_add(1);
        usage.bytes = usage.bytes.saturating_add(content_metadata.len());
    }
    Ok(usage)
}

fn is_hashed_file_directory(name: &str) -> bool {
    name.len() == 29
        && name.starts_with("file-")
        && name[5..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn load_manifest(destination_root: &Path) -> AppResult<PersonaIngestManifest> {
    let manifest_path = destination_root.join(MANIFEST_FILE_NAME);
    require_under_root(&manifest_path, destination_root, "ingest manifest")?;
    // codeql[rust/path-injection]
    let metadata = match fs::symlink_metadata(&manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersonaIngestManifest::default())
        }
        Err(error) => return Err(filesystem_error("inspect the app-owned ingest manifest", error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::Validation(
            "App-owned ingest manifest must be a regular file".to_string(),
        ));
    }
    // codeql[rust/path-injection]
    let encoded = fs::read(&manifest_path)
        .map_err(|error| filesystem_error("read the app-owned ingest manifest", error))?;
    serde_json::from_slice(&encoded).map_err(|error| {
        AppError::Infrastructure(format!("Failed to parse ingest manifest: {error}"))
    })
}

fn merge_manifest(
    cumulative: &mut PersonaIngestManifest,
    batch: &PersonaIngestManifest,
) {
    merge_entries(&mut cumulative.copied, &batch.copied);
    merge_entries(&mut cumulative.skipped, &batch.skipped);
    merge_entries(&mut cumulative.rejected, &batch.rejected);
}

fn merge_entries(cumulative: &mut Vec<PersonaIngestEntry>, batch: &[PersonaIngestEntry]) {
    let mut counts = cumulative.iter().fold(HashMap::new(), |mut counts, entry| {
        *counts.entry((entry.path.clone(), entry.reason.clone())).or_insert(0usize) += 1;
        counts
    });
    let mut batch_counts = HashMap::new();
    for entry in batch {
        let key = (entry.path.clone(), entry.reason.clone());
        let batch_count = batch_counts.entry(key.clone()).or_insert(0usize);
        *batch_count += 1;
        let cumulative_count = counts.entry(key).or_insert(0usize);
        if *cumulative_count < *batch_count {
            cumulative.push(entry.clone());
            *cumulative_count += 1;
        }
    }
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
