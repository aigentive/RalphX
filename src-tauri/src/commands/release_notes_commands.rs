use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, Runtime, State};

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
pub async fn get_current_release_notes<R: Runtime>(
    app: AppHandle<R>,
) -> Result<ReleaseNotesResponse, String> {
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

fn read_release_notes_for_version<R: Runtime>(
    app: &AppHandle<R>,
    version: &str,
) -> Result<ReleaseNotesResponse, String> {
    let filename = release_notes_filename(version)?;
    let candidates = release_notes_candidates(app, &filename);
    Ok(read_release_notes_from_candidates(version, candidates))
}

fn release_notes_candidates<R: Runtime>(
    app: &AppHandle<R>,
    filename: &str,
) -> Vec<(PathBuf, ReleaseNotesSource)> {
    let resource_dir = app.path().resource_dir().ok();
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|path| path.to_path_buf());

    release_notes_candidates_from_roots(resource_dir, repo_root, filename)
}

fn release_notes_candidates_from_roots(
    resource_dir: Option<PathBuf>,
    repo_root: Option<PathBuf>,
    filename: &str,
) -> Vec<(PathBuf, ReleaseNotesSource)> {
    let mut candidates = Vec::new();

    if let Some(resource_dir) = resource_dir {
        candidates.push((
            resource_dir.join(RELEASE_NOTES_DIR).join(filename),
            ReleaseNotesSource::BundledResource,
        ));
    }

    if let Some(repo_root) = repo_root {
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
    read_release_notes_from_candidates_with_reader(version, candidates, |path| {
        std::fs::read_to_string(path)
    })
}

fn read_release_notes_from_candidates_with_reader(
    version: &str,
    candidates: Vec<(PathBuf, ReleaseNotesSource)>,
    mut read_file: impl FnMut(&Path) -> std::io::Result<String>,
) -> ReleaseNotesResponse {
    for (path, source) in candidates {
        if let Ok(body) = read_file(&path) {
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
