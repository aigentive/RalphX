// Standalone chat workspace service.
//
// Generalizes the containment discipline from `persona_ingest.rs` for a per-conversation,
// app-owned filesystem workspace. Phase 4a.2 wires this in for `ChatContextType::Standalone`
// conversations only; the same root/manifest shape is intended to also back
// `AgentConversationWorkspaceMode::PersonaBuilder` conversations that need a private working
// directory in Phase 5 (deferred — see `docs/handoffs/personas-v2-builder-scoping-artifacts.md`).
//
// Root layout: `<app_data_dir>/standalone_workspaces/conversation-<sha256(conversation_id)[:12]>/`
// The directory name is hash-derived so the raw conversation id is NEVER used as a path
// component (CodeQL path-injection rule: hash or enum untrusted path components). A small
// app-owned manifest (`manifest.json`) inside the workspace records the original conversation
// id + creation timestamp, mirroring `persona_ingest`'s manifest pattern; the crash-orphan
// sweep uses it to recover the conversation id from a hash-derived directory name.
//
// Lifecycle: archiving a Standalone conversation NEVER deletes its workspace. The only
// reclamation path is `sweep_orphaned_standalone_workspaces`, a startup job that deletes
// workspace directories whose conversation id no longer has a row in the DB. The sweep is
// fail-safe by construction: any entry it cannot positively prove is orphaned (symlink,
// unreadable/invalid manifest, or a repository lookup error) is left untouched.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::entities::ChatConversationId;
use crate::domain::repositories::ChatConversationRepository;
use crate::error::{AppError, AppResult};

use super::persona_ingest::{filesystem_error, require_under_root};

const STANDALONE_WORKSPACES_DIR: &str = "standalone_workspaces";
const MANIFEST_FILE_NAME: &str = "manifest.json";

/// Manifest persisted inside each standalone workspace directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StandaloneWorkspaceManifest {
    conversation_id: String,
    created_at: DateTime<Utc>,
}

/// Returns the app-owned root under which all standalone workspaces live.
pub fn standalone_workspaces_root(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(STANDALONE_WORKSPACES_DIR)
}

/// Returns the hash-derived workspace directory for one conversation, without creating
/// it. `root` should already be the canonicalized workspaces root.
pub fn standalone_workspace_path(root: &Path, conversation_id: &str) -> PathBuf {
    root.join(hashed_conversation_component(conversation_id))
}

fn hashed_conversation_component(conversation_id: &str) -> String {
    let digest = Sha256::digest(conversation_id.as_bytes());
    let encoded = digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("conversation-{encoded}")
}

/// Idempotently ensures a private, app-owned workspace directory exists for
/// `conversation_id` and returns its canonical path.
///
/// Safe under concurrent calls: `fs::create_dir_all` is a no-op when the directory
/// already exists, and the manifest is only written the first time it is missing.
///
/// # Errors
/// Returns an error when the app-owned root or workspace directory cannot be
/// created/canonicalized, or when an existing entry at the workspace path is a
/// symlink (never trusted, never followed).
pub fn ensure_workspace(app_data_dir: &Path, conversation_id: &str) -> AppResult<PathBuf> {
    let root = standalone_workspaces_root(app_data_dir);
    // codeql[rust/path-injection]
    fs::create_dir_all(&root)
        .map_err(|error| filesystem_error("create the standalone workspaces root", error))?;
    // codeql[rust/path-injection]
    let canonical_root = root
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize the standalone workspaces root", error))?;

    let workspace_path = standalone_workspace_path(&canonical_root, conversation_id);
    require_under_root(&workspace_path, &canonical_root, "standalone workspace")?;

    if fs::symlink_metadata(&workspace_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(
            "Standalone workspace path must not be a symlink".to_string(),
        ));
    }

    // codeql[rust/path-injection]
    fs::create_dir_all(&workspace_path)
        .map_err(|error| filesystem_error("create the standalone workspace directory", error))?;
    // codeql[rust/path-injection]
    let canonical_workspace = workspace_path.canonicalize().map_err(|error| {
        filesystem_error("canonicalize the standalone workspace directory", error)
    })?;
    require_under_root(
        &canonical_workspace,
        &canonical_root,
        "standalone workspace",
    )?;

    write_manifest_if_missing(&canonical_workspace, conversation_id)?;

    Ok(canonical_workspace)
}

fn write_manifest_if_missing(workspace_root: &Path, conversation_id: &str) -> AppResult<()> {
    let manifest_path = workspace_root.join(MANIFEST_FILE_NAME);
    require_under_root(
        &manifest_path,
        workspace_root,
        "standalone workspace manifest",
    )?;
    if fs::symlink_metadata(&manifest_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Err(AppError::Validation(
            "Standalone workspace manifest must not be a symlink".to_string(),
        ));
    }
    if manifest_path.is_file() {
        return Ok(());
    }

    let manifest = StandaloneWorkspaceManifest {
        conversation_id: conversation_id.to_string(),
        created_at: Utc::now(),
    };
    let encoded = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        AppError::Infrastructure(format!(
            "Failed to serialize standalone workspace manifest: {error}"
        ))
    })?;
    // codeql[rust/path-injection]
    fs::write(&manifest_path, encoded)
        .map_err(|error| filesystem_error("write the standalone workspace manifest", error))
}

/// Summary of one crash-orphan sweep pass, returned for logging/tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StandaloneWorkspaceSweepSummary {
    pub removed: usize,
    pub retained: usize,
    pub skipped: usize,
}

/// Deletes standalone workspace directories whose conversation id no longer has a row in
/// `chat_conversation_repo`. This is the ONLY reclamation path — archiving a Standalone
/// conversation never deletes its workspace, only this startup sweep does, and only for
/// entries it can positively prove are orphaned.
pub async fn sweep_orphaned_standalone_workspaces(
    app_data_dir: &Path,
    chat_conversation_repo: Arc<dyn ChatConversationRepository>,
) -> StandaloneWorkspaceSweepSummary {
    let mut summary = StandaloneWorkspaceSweepSummary::default();
    let root = standalone_workspaces_root(app_data_dir);
    // codeql[rust/path-injection]
    let Ok(canonical_root) = root.canonicalize() else {
        // Root does not exist yet (no standalone workspace has ever been created) —
        // nothing to sweep.
        return summary;
    };
    // codeql[rust/path-injection]
    let Ok(entries) = fs::read_dir(&canonical_root) else {
        return summary;
    };

    for entry in entries.flatten() {
        let entry_path = entry.path();
        if !is_safe_sweep_candidate(&entry_path, &canonical_root) {
            summary.skipped += 1;
            continue;
        }

        let Some(conversation_id) = read_manifest_conversation_id(&entry_path) else {
            summary.skipped += 1;
            continue;
        };

        match chat_conversation_repo
            .get_by_id(&ChatConversationId::from_string(conversation_id))
            .await
        {
            Ok(Some(_)) => summary.retained += 1,
            Ok(None) => {
                if remove_workspace_entry(&entry_path, &canonical_root).is_ok() {
                    summary.removed += 1;
                } else {
                    summary.skipped += 1;
                }
            }
            Err(error) => {
                // Fail closed: a repository error is not proof of absence, so the
                // entry must be preserved, never deleted.
                tracing::warn!(
                    error = %error,
                    path = %entry_path.display(),
                    "Skipping standalone workspace sweep entry after repository lookup error"
                );
                summary.skipped += 1;
            }
        }
    }

    summary
}

/// True when `entry_path` is a non-symlink directory that stays under `canonical_root`.
/// Symlinked entries are never trusted — the sweep must not follow them to decide what
/// to delete or descend into their target.
fn is_safe_sweep_candidate(entry_path: &Path, canonical_root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(entry_path) else {
        return false;
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return false;
    }
    require_under_root(
        entry_path,
        canonical_root,
        "standalone workspace sweep entry",
    )
    .is_ok()
}

fn read_manifest_conversation_id(entry_path: &Path) -> Option<String> {
    let manifest_path = entry_path.join(MANIFEST_FILE_NAME);
    if fs::symlink_metadata(&manifest_path)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(true)
    {
        return None;
    }
    // codeql[rust/path-injection]
    let contents = fs::read_to_string(&manifest_path).ok()?;
    let manifest: StandaloneWorkspaceManifest = serde_json::from_str(&contents).ok()?;
    Some(manifest.conversation_id)
}

fn remove_workspace_entry(entry_path: &Path, canonical_root: &Path) -> AppResult<()> {
    // codeql[rust/path-injection]
    let canonical_entry = entry_path
        .canonicalize()
        .map_err(|error| filesystem_error("canonicalize a swept workspace entry", error))?;
    require_under_root(
        &canonical_entry,
        canonical_root,
        "standalone workspace sweep entry",
    )?;
    // codeql[rust/path-injection]
    fs::remove_dir_all(&canonical_entry)
        .map_err(|error| filesystem_error("remove an orphaned standalone workspace", error))
}
