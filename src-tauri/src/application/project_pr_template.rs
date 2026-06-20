use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use ralphx_domain::error::{AppError, AppResult};

const GITHUB_DIR: &str = ".github";
const PREFERRED_PR_TEMPLATE_FILE: &str = "pull_request_template.md";
const LEGACY_PR_TEMPLATE_FILE: &str = "PULL_REQUEST_TEMPLATE.md";
const PR_TEMPLATE_FILES: [&str; 2] = [PREFERRED_PR_TEMPLATE_FILE, LEGACY_PR_TEMPLATE_FILE];

pub fn read_pr_template(project_root: &Path) -> AppResult<Option<String>> {
    let root = canonical_project_root(project_root)?;
    let github_dir = root.join(GITHUB_DIR);
    let Some(github_dir) = existing_safe_github_dir(&root, &github_dir)? else {
        return Ok(None);
    };

    let Some(template_path) = existing_safe_template_file(&github_dir)? else {
        return Ok(None);
    };

    // codeql[rust/path-injection]: template_path is fixed under canonical project root and rejects symlinks/non-files above.
    fs::read_to_string(&template_path)
        .map(Some)
        .map_err(|error| AppError::Infrastructure(format!("Failed to read PR template: {error}")))
}

pub fn write_pr_template(project_root: &Path, content: &str) -> AppResult<()> {
    let root = canonical_project_root(project_root)?;
    let github_dir = root.join(GITHUB_DIR);
    let github_dir = ensure_safe_github_dir(&root, &github_dir)?;
    let template_path = existing_safe_template_file(&github_dir)?
        .unwrap_or_else(|| github_dir.join(PREFERRED_PR_TEMPLATE_FILE));

    if let Some(metadata) = symlink_metadata_optional(&template_path)? {
        if metadata.file_type().is_symlink() {
            return Err(AppError::Validation(
                "PR template path must not be a symlink".to_string(),
            ));
        }
        if !metadata.is_file() {
            return Err(AppError::Validation(
                "PR template path must be a regular file".to_string(),
            ));
        }
    }

    let parent_metadata = fs::symlink_metadata(&github_dir).map_err(|error| {
        AppError::Infrastructure(format!("Failed to inspect .github directory: {error}"))
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(AppError::Validation(
            ".github must be a regular directory".to_string(),
        ));
    }
    ensure_under_root(
        &root,
        &github_dir.canonicalize().map_err(|error| {
            AppError::Infrastructure(format!("Failed to canonicalize .github directory: {error}"))
        })?,
    )?;

    // codeql[rust/path-injection]: template_path is a fixed filename in a canonicalized, symlink-free .github directory under the project root.
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&template_path)
        .map_err(|error| {
            AppError::Infrastructure(format!("Failed to open PR template for writing: {error}"))
        })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        AppError::Infrastructure(format!("Failed to write PR template: {error}"))
    })?;
    Ok(())
}

fn canonical_project_root(project_root: &Path) -> AppResult<PathBuf> {
    if !project_root.is_absolute() || project_root.parent().is_none() {
        return Err(AppError::Validation(
            "project working directory must be an absolute non-root path".to_string(),
        ));
    }

    // codeql[rust/path-injection]: project_root was validated by project command resolution and is canonicalized before child sinks.
    let root = project_root.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!("Failed to canonicalize project root: {error}"))
    })?;
    if !root.is_dir() {
        return Err(AppError::Validation(
            "project working directory must be a directory".to_string(),
        ));
    }
    Ok(root)
}

fn existing_safe_github_dir(root: &Path, github_dir: &Path) -> AppResult<Option<PathBuf>> {
    let Some(metadata) = symlink_metadata_optional(github_dir)? else {
        return Ok(None);
    };
    if metadata.file_type().is_symlink() {
        return Err(AppError::Validation(
            ".github must not be a symlink".to_string(),
        ));
    }
    if !metadata.is_dir() {
        return Err(AppError::Validation(
            ".github must be a regular directory".to_string(),
        ));
    }
    // codeql[rust/path-injection]: github_dir is the fixed .github child of a canonicalized project root.
    let canonical = github_dir.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!("Failed to canonicalize .github directory: {error}"))
    })?;
    ensure_under_root(root, &canonical)?;
    Ok(Some(canonical))
}

fn ensure_safe_github_dir(root: &Path, github_dir: &Path) -> AppResult<PathBuf> {
    validate_fixed_component(GITHUB_DIR)?;
    if let Some(existing) = existing_safe_github_dir(root, github_dir)? {
        return Ok(existing);
    }

    // codeql[rust/path-injection]: github_dir is a validated fixed child component under canonical project root.
    fs::create_dir(github_dir).map_err(|error| {
        AppError::Infrastructure(format!("Failed to create .github directory: {error}"))
    })?;
    existing_safe_github_dir(root, github_dir)?.ok_or_else(|| {
        AppError::Infrastructure("Failed to verify created .github directory".to_string())
    })
}

fn existing_safe_template_file(github_dir: &Path) -> AppResult<Option<PathBuf>> {
    let Some(template_path) = exact_template_path(github_dir)? else {
        return Ok(None);
    };
    let metadata = fs::symlink_metadata(&template_path).map_err(|error| {
        AppError::Infrastructure(format!("Failed to inspect PR template: {error}"))
    })?;
    if metadata.file_type().is_symlink() {
        return Err(AppError::Validation(
            "PR template path must not be a symlink".to_string(),
        ));
    }
    if !metadata.is_file() {
        return Err(AppError::Validation(
            "PR template path must be a regular file".to_string(),
        ));
    }
    // codeql[rust/path-injection]: template_path is an exact directory entry matching a validated fixed filename under canonical .github.
    let canonical = template_path.canonicalize().map_err(|error| {
        AppError::Infrastructure(format!("Failed to canonicalize PR template: {error}"))
    })?;
    ensure_under_root(github_dir, &canonical)?;
    Ok(Some(canonical))
}

fn exact_template_path(github_dir: &Path) -> AppResult<Option<PathBuf>> {
    for filename in PR_TEMPLATE_FILES {
        validate_fixed_component(filename)?;
    }

    let entries = fs::read_dir(github_dir).map_err(|error| {
        AppError::Infrastructure(format!("Failed to list .github directory: {error}"))
    })?;
    let mut preferred = None;
    let mut legacy = None;
    for entry in entries {
        let entry = entry.map_err(|error| {
            AppError::Infrastructure(format!("Failed to inspect .github entry: {error}"))
        })?;
        let file_name = entry.file_name();
        if file_name == PREFERRED_PR_TEMPLATE_FILE {
            preferred = Some(entry.path());
        } else if file_name == LEGACY_PR_TEMPLATE_FILE {
            legacy = Some(entry.path());
        }
    }

    Ok(preferred.or(legacy))
}

fn symlink_metadata_optional(path: &Path) -> AppResult<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::Infrastructure(format!(
            "Failed to inspect path: {error}"
        ))),
    }
}

fn validate_fixed_component(component: &str) -> AppResult<()> {
    let mut components = Path::new(component).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(AppError::Validation(
            "PR template path contains an invalid fixed component".to_string(),
        )),
    }
}

fn ensure_under_root(root: &Path, child: &Path) -> AppResult<()> {
    if child.starts_with(root) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "PR template path escapes project working directory".to_string(),
        ))
    }
}
