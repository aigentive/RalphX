use std::path::PathBuf;

use super::*;
use crate::infrastructure::sqlite::get_default_db_path;

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
    assert_eq!(
        paths.workflow_runtime_dir(),
        paths.app_data_dir().join("workflow-runtime")
    );
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

#[test]
fn global_router_path_uses_explicit_ralphx_config_root() {
    let paths =
        AppPaths::new_with_config_dir("/tmp/ralphx-app-data", None, "/tmp/user-config/.ralphx");

    assert_eq!(
        paths.global_router_path(),
        PathBuf::from("/tmp/user-config/.ralphx/router.yaml")
    );
}

#[test]
fn global_mcp_policy_path_uses_explicit_ralphx_config_root() {
    let paths =
        AppPaths::new_with_config_dir("/tmp/ralphx-app-data", None, "/tmp/user-config/.ralphx");

    assert_eq!(
        paths.global_mcp_policy_path(),
        PathBuf::from("/tmp/user-config/.ralphx/mcp.yaml")
    );
}

#[test]
fn database_maintenance_dir_is_under_app_data_dir() {
    let paths = AppPaths::new("/tmp/ralphx-app-data", None);

    assert_eq!(
        paths.database_maintenance_dir(),
        PathBuf::from("/tmp/ralphx-app-data/database-maintenance")
    );
}

#[test]
fn database_compaction_marker_path_is_under_maintenance_dir() {
    let paths = AppPaths::new("/tmp/ralphx-app-data", None);

    assert_eq!(
        paths.database_compaction_marker_path(),
        PathBuf::from("/tmp/ralphx-app-data/database-maintenance/compact-on-next-launch")
    );
}

#[test]
fn database_backup_dir_is_under_maintenance_dir() {
    let paths = AppPaths::new("/tmp/ralphx-app-data", None);

    assert_eq!(
        paths.database_backup_dir(),
        PathBuf::from("/tmp/ralphx-app-data/database-maintenance/backups")
    );
}

#[test]
fn database_maintenance_paths_resolves_all_members() {
    let paths = AppPaths::new("/tmp/ralphx-app-data", None);
    let maint = paths.database_maintenance_paths().unwrap();

    assert_eq!(maint.marker_path, paths.database_compaction_marker_path());
    assert_eq!(maint.backup_dir, paths.database_backup_dir());
    assert_eq!(maint.database_path, paths.database_path().unwrap());
}
