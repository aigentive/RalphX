use std::path::{Component, Path, PathBuf};

use crate::error::{AppError, AppResult};

/// Minimal lexical guard for filesystem sinks that receive paths already scoped by
/// higher-level project/worktree logic.
pub fn validate_absolute_non_root_path(path: &Path, context: &str) -> AppResult<PathBuf> {
    if !path.is_absolute() {
        return Err(AppError::Validation(format!(
            "{context} path must be absolute: {}",
            path.display()
        )));
    }

    let mut normalized = PathBuf::new();
    let mut normal_components = 0usize;

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Normal(part) => {
                normal_components += 1;
                normalized.push(part);
            }
            Component::ParentDir | Component::CurDir => {
                return Err(AppError::Validation(format!(
                    "{context} path contains unsafe components: {}",
                    path.display()
                )));
            }
        }
    }

    if normal_components == 0 {
        return Err(AppError::Validation(format!(
            "{context} path must not be a filesystem root: {}",
            path.display()
        )));
    }

    Ok(normalized)
}

pub fn checked_exists(path: &Path, context: &str) -> AppResult<bool> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    Ok(safe_path.exists())
}

pub fn checked_is_file(path: &Path, context: &str) -> AppResult<bool> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    Ok(safe_path.is_file())
}

pub fn checked_is_symlink(path: &Path, context: &str) -> AppResult<bool> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    Ok(safe_path.is_symlink())
}

pub fn checked_read_to_string(path: &Path, context: &str) -> AppResult<String> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    std::fs::read_to_string(&safe_path).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to read {context} file {}: {e}",
            safe_path.display()
        ))
    })
}

pub fn checked_remove_file(path: &Path, context: &str) -> AppResult<()> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    std::fs::remove_file(&safe_path).map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to remove {context} file {}: {e}",
            safe_path.display()
        ))
    })
}

pub async fn checked_remove_dir_all(path: &Path, context: &str) -> AppResult<()> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    tokio::fs::remove_dir_all(&safe_path).await.map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to remove {context} directory {}: {e}",
            safe_path.display()
        ))
    })
}

pub async fn checked_read_dir(path: &Path, context: &str) -> AppResult<tokio::fs::ReadDir> {
    let safe_path = validate_absolute_non_root_path(path, context)?;

    tokio::fs::read_dir(&safe_path).await.map_err(|e| {
        AppError::Infrastructure(format!(
            "Failed to read {context} directory {}: {e}",
            safe_path.display()
        ))
    })
}

pub(crate) fn require_under_root(path: &Path, root: &Path, label: &str) -> AppResult<()> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "{label} escapes its allowed root"
        )))
    }
}

pub(crate) fn filesystem_error(action: &str, error: std::io::Error) -> AppError {
    AppError::Infrastructure(format!("Failed to {action}: {error}"))
}

#[cfg(test)]
#[path = "path_safety_tests.rs"]
mod tests;
