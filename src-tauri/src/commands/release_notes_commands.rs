use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

use crate::application::AppState;

const RELEASE_NOTES_DIR: &str = "release-notes";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseNotesSource {
    BundledResource,
    DevelopmentCheckout,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotesResponse {
    pub version: String,
    pub body: Option<String>,
    pub source: ReleaseNotesSource,
}

#[tauri::command]
pub async fn get_current_release_notes(app: AppHandle) -> Result<ReleaseNotesResponse, String> {
    let version = app.package_info().version.to_string();
    Ok(read_release_notes_for_version(&app, &version)?)
}

#[tauri::command]
pub async fn get_last_seen_release_notes_version(
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    state
        .app_state_repo
        .get()
        .await
        .map(|settings| settings.last_seen_release_notes_version)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn mark_release_notes_seen(
    version: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let version = sanitize_release_notes_version(&version)?;
    state
        .app_state_repo
        .set_last_seen_release_notes_version(Some(&version))
        .await
        .map_err(|error| error.to_string())
}

fn read_release_notes_for_version(
    app: &AppHandle,
    version: &str,
) -> Result<ReleaseNotesResponse, String> {
    let filename = release_notes_filename(version)?;
    let candidates = release_notes_candidates(app, &filename);
    Ok(read_release_notes_from_candidates(version, candidates))
}

fn release_notes_candidates(app: &AppHandle, filename: &str) -> Vec<(PathBuf, ReleaseNotesSource)> {
    let mut candidates = Vec::new();

    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push((
            resource_dir.join(RELEASE_NOTES_DIR).join(filename),
            ReleaseNotesSource::BundledResource,
        ));
    }

    if let Some(repo_root) = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent() {
        candidates.push((
            repo_root.join(RELEASE_NOTES_DIR).join(filename),
            ReleaseNotesSource::DevelopmentCheckout,
        ));
    }

    candidates
}

fn read_release_notes_from_candidates(
    version: &str,
    candidates: Vec<(PathBuf, ReleaseNotesSource)>,
) -> ReleaseNotesResponse {
    for (path, source) in candidates {
        if let Ok(body) = std::fs::read_to_string(path) {
            return ReleaseNotesResponse {
                version: version.to_string(),
                body: Some(body),
                source,
            };
        }
    }

    ReleaseNotesResponse {
        version: version.to_string(),
        body: None,
        source: ReleaseNotesSource::Missing,
    }
}

fn release_notes_filename(version: &str) -> Result<String, String> {
    sanitize_release_notes_version(version).map(|version| format!("v{version}.md"))
}

fn sanitize_release_notes_version(version: &str) -> Result<String, String> {
    let version = version.trim().trim_start_matches('v');
    if version.is_empty() {
        return Err("Release notes version cannot be empty".to_string());
    }
    if version.contains("..")
        || version.contains('/')
        || version.contains('\\')
        || !version
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    {
        return Err("Invalid release notes version".to_string());
    }

    Ok(version.to_string())
}

#[cfg(test)]
#[path = "release_notes_commands_tests.rs"]
mod tests;
