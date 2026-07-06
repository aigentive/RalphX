use std::path::{Path, PathBuf};

use tauri::Manager;

use crate::application::chat_attachment_storage::chat_attachment_storage_path;
use crate::error::{AppError, AppResult};
use crate::infrastructure::sqlite::{get_app_data_db_path, get_default_db_path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub app_data_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
}

impl AppPaths {
    pub fn new(app_data_dir: impl Into<PathBuf>, resource_dir: Option<PathBuf>) -> Self {
        Self {
            app_data_dir: app_data_dir.into(),
            resource_dir,
        }
    }

    pub fn from_app_handle(app_handle: &tauri::AppHandle) -> AppResult<Self> {
        let app_data_dir = app_handle.path().app_data_dir().map_err(|error| {
            AppError::Infrastructure(format!("Failed to resolve app data dir: {error}"))
        })?;
        let resource_dir = app_handle.path().resource_dir().ok();

        Ok(Self::new(app_data_dir, resource_dir))
    }

    pub fn for_tests() -> Self {
        Self::new(std::env::temp_dir().join("ralphx-test-app-data"), None)
    }

    pub fn database_path(&self) -> AppResult<PathBuf> {
        if cfg!(debug_assertions) {
            Ok(get_default_db_path())
        } else {
            get_app_data_db_path(&self.app_data_dir)
        }
    }

    pub fn attachment_storage_path(&self) -> PathBuf {
        chat_attachment_storage_path(&self.app_data_dir)
    }

    pub fn app_data_dir(&self) -> &Path {
        &self.app_data_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_storage_path_uses_app_data_dir() {
        let app_data_dir = PathBuf::from("/tmp/ralphx-app-paths-test");
        let paths = AppPaths::new(app_data_dir.clone(), None);

        assert_eq!(
            paths.attachment_storage_path(),
            app_data_dir.join("attachments")
        );
    }
}
