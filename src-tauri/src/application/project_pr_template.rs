use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use ralphx_domain::error::{AppError, AppResult};

const GITHUB_DIR: &str = ".github";
const PREFERRED_PR_TEMPLATE_FILE: &str = "pull_request_template.md";
const LEGACY_PR_TEMPLATE_FILE: &str = "PULL_REQUEST_TEMPLATE.md";
const PR_TEMPLATE_FILES: [&str; 2] = [PREFERRED_PR_TEMPLATE_FILE, LEGACY_PR_TEMPLATE_FILE];

pub fn read_pr_template(project_root: &Path) -> AppResult<Option<String>> {
    let root = canonical_project_root(project_root)?;
    let root_dir = open_project_root(&root)?;
    let Some(_github_dir) = existing_safe_github_dir(&root_dir)? else {
        return Ok(None);
    };

    let Some(template_file) = existing_safe_template_file(&root_dir)? else {
        return Ok(None);
    };

    root_dir
        .read_to_string(template_relative_path(template_file))
        .map(Some)
        .map_err(|error| AppError::Infrastructure(format!("Failed to read PR template: {error}")))
}

pub fn write_pr_template(project_root: &Path, content: &str) -> AppResult<()> {
    let root = canonical_project_root(project_root)?;
    let root_dir = open_project_root(&root)?;
    let _github_dir = ensure_safe_github_dir(&root_dir)?;
    let template_file =
        existing_safe_template_file(&root_dir)?.unwrap_or(PREFERRED_PR_TEMPLATE_FILE);
    let template_path = template_relative_path(template_file);

    let parent_metadata = root_dir.symlink_metadata(GITHUB_DIR).map_err(|error| {
        AppError::Infrastructure(format!("Failed to inspect .github directory: {error}"))
    })?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        return Err(AppError::Validation(
            ".github must be a regular directory".to_string(),
        ));
    }

    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    let mut file = root_dir
        .open_with(&template_path, &options)
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

    // codeql[rust/path-injection]
    let root = dunce::canonicalize(project_root).map_err(|error| {
        AppError::Infrastructure(format!("Failed to canonicalize project root: {error}"))
    })?;
    if !root.is_dir() {
        return Err(AppError::Validation(
            "project working directory must be a directory".to_string(),
        ));
    }
    Ok(root)
}

fn open_project_root(root: &Path) -> AppResult<Dir> {
    Dir::open_ambient_dir(root, ambient_authority()).map_err(|error| {
        AppError::Infrastructure(format!("Failed to open project root directory: {error}"))
    })
}

fn existing_safe_github_dir(root: &Dir) -> AppResult<Option<Dir>> {
    let Some(metadata) = symlink_metadata_optional(root, GITHUB_DIR)? else {
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
    root.open_dir(GITHUB_DIR).map(Some).map_err(|error| {
        AppError::Infrastructure(format!("Failed to open .github directory: {error}"))
    })
}

fn ensure_safe_github_dir(root: &Dir) -> AppResult<Dir> {
    validate_fixed_component(GITHUB_DIR)?;
    if let Some(existing) = existing_safe_github_dir(root)? {
        return Ok(existing);
    }

    root.create_dir(GITHUB_DIR).map_err(|error| {
        AppError::Infrastructure(format!("Failed to create .github directory: {error}"))
    })?;
    existing_safe_github_dir(root)?.ok_or_else(|| {
        AppError::Infrastructure("Failed to verify created .github directory".to_string())
    })
}

fn existing_safe_template_file(root: &Dir) -> AppResult<Option<&'static str>> {
    let Some(template_file) = exact_template_file(root)? else {
        return Ok(None);
    };
    let template_path = template_relative_path(template_file);
    let metadata = root.symlink_metadata(&template_path).map_err(|error| {
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
    Ok(Some(template_file))
}

fn exact_template_file(root: &Dir) -> AppResult<Option<&'static str>> {
    for filename in PR_TEMPLATE_FILES {
        validate_fixed_component(filename)?;
    }

    let entries = root.read_dir(GITHUB_DIR).map_err(|error| {
        AppError::Infrastructure(format!("Failed to list .github directory: {error}"))
    })?;
    let mut preferred = None;
    let mut legacy = None;
    for entry in entries {
        let entry = entry.map_err(|error| {
            AppError::Infrastructure(format!("Failed to inspect .github entry: {error}"))
        })?;
        let file_name = entry.file_name();
        if file_name == OsStr::new(PREFERRED_PR_TEMPLATE_FILE) {
            preferred = Some(PREFERRED_PR_TEMPLATE_FILE);
        } else if file_name == OsStr::new(LEGACY_PR_TEMPLATE_FILE) {
            legacy = Some(LEGACY_PR_TEMPLATE_FILE);
        }
    }

    Ok(preferred.or(legacy))
}

fn symlink_metadata_optional(root: &Dir, path: &str) -> AppResult<Option<cap_std::fs::Metadata>> {
    match root.symlink_metadata(path) {
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

fn template_relative_path(filename: &str) -> PathBuf {
    Path::new(GITHUB_DIR).join(filename)
}
