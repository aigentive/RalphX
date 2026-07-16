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

    pub(crate) fn database_path_for_profile(&self, debug_assertions: bool) -> AppResult<PathBuf> {
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

    pub fn workflow_runtime_dir(&self) -> PathBuf {
        self.app_data_dir.join("workflow-runtime")
    }

    pub fn workflow_runner_path(&self) -> AppResult<PathBuf> {
        let executable = std::env::current_exe().map_err(|error| {
            AppError::Infrastructure(format!("Failed to resolve RalphX executable: {error}"))
        })?;
        let parent = executable.parent().ok_or_else(|| {
            AppError::Infrastructure("RalphX executable has no parent directory".into())
        })?;
        Ok(parent.join(if cfg!(windows) {
            "ralphx-workflow-runner.exe"
        } else {
            "ralphx-workflow-runner"
        }))
    }
}
