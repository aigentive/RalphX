use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const PERSONA_INGEST_DIR: &str = "persona_ingest";

/// Returns the app-owned root used by legacy copied persona source material.
pub fn persona_ingest_storage_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(PERSONA_INGEST_DIR)
}

/// Returns a hash-addressed legacy ingest root for one conversation.
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

/// Returns a validated non-empty legacy ingest root when an old builder conversation
/// still has copied context. This read-only compatibility path is intentionally permanent.
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

fn hashed_component(prefix: &str, value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let encoded = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}-{encoded}")
}

#[cfg(test)]
#[path = "persona_ingest_tests.rs"]
mod tests;
