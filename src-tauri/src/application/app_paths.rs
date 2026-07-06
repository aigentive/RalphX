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
        self.database_path_for_profile(cfg!(debug_assertions))
    }

    fn database_path_for_profile(&self, debug_assertions: bool) -> AppResult<PathBuf> {
        if debug_assertions {
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

    #[test]
    fn for_tests_uses_temp_app_data_dir_without_resources() {
        let paths = AppPaths::for_tests();

        assert!(paths.app_data_dir().ends_with("ralphx-test-app-data"));
        assert_eq!(paths.resource_dir, None);
    }

    #[test]
    fn database_path_uses_default_db_path_for_debug_profile() {
        let paths = AppPaths::new("/tmp/ralphx-app-data", None);

        assert_eq!(
            paths.database_path_for_profile(true).expect("debug path"),
            get_default_db_path()
        );
        assert_eq!(
            paths.database_path().expect("current profile path"),
            get_default_db_path()
        );
    }

    #[test]
    fn database_path_uses_app_data_dir_for_release_profile() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let app_data_dir = temp_dir.path().join("app-data");
        let paths = AppPaths::new(app_data_dir.clone(), None);

        let db_path = paths
            .database_path_for_profile(false)
            .expect("release profile path");

        assert!(app_data_dir.exists());
        assert_eq!(db_path, app_data_dir.join("ralphx.db"));
    }
}
