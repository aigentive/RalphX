use crate::shell::app_setup::{
    configure_bundled_runtime_paths, generated_plugin_dir_for_app_data,
    generated_plugin_runtime_profile_component, resolve_bundled_runtime_paths, BundledRuntimePaths,
};
use tempfile::tempdir;

#[test]
fn bundled_runtime_profile_uses_debug_for_test_builds() {
    assert_eq!(generated_plugin_runtime_profile_component(), "debug");
}

#[test]
fn bundled_runtime_paths_require_plugin_and_agents_directories() {
    let temp = tempdir().expect("tempdir");
    let resource_dir = temp.path().join("Resources");
    let app_data_dir = temp.path().join("AppData");

    std::fs::create_dir_all(resource_dir.join("plugins/app")).expect("plugin dir");
    assert!(
        resolve_bundled_runtime_paths(&resource_dir, &app_data_dir).is_none(),
        "bundled runtime should not resolve without canonical agents"
    );

    std::fs::create_dir_all(resource_dir.join("agents")).expect("agents dir");
    let paths =
        resolve_bundled_runtime_paths(&resource_dir, &app_data_dir).expect("bundled runtime paths");

    assert_eq!(paths.plugin_dir, resource_dir.join("plugins/app"));
    assert_eq!(paths.config_dir, None);
    assert_eq!(
        paths.generated_plugin_dir,
        generated_plugin_dir_for_app_data(&app_data_dir)
    );
    assert!(paths
        .generated_plugin_dir
        .starts_with(app_data_dir.join("generated")));
    assert!(paths.generated_plugin_dir.ends_with("claude-plugin"));

    std::fs::create_dir_all(resource_dir.join("config")).expect("config dir");
    let paths = resolve_bundled_runtime_paths(&resource_dir, &app_data_dir)
        .expect("bundled runtime paths with config");
    assert_eq!(paths.config_dir, Some(resource_dir.join("config")));
}

#[test]
fn bundled_runtime_path_configuration_forwards_optional_config_dir() {
    let temp = tempdir().expect("tempdir");
    let paths = BundledRuntimePaths {
        plugin_dir: temp.path().join("Resources/plugins/app"),
        config_dir: Some(temp.path().join("Resources/config")),
        generated_plugin_dir: temp.path().join("AppData/generated/claude-plugin"),
    };

    let mut configured_plugin_dirs = None;
    let mut configured_config_dir = None;
    let did_configure_config_dir = configure_bundled_runtime_paths(
        paths.clone(),
        |plugin_dir, generated_plugin_dir| {
            configured_plugin_dirs = Some((plugin_dir, generated_plugin_dir));
        },
        |config_dir| {
            configured_config_dir = Some(config_dir);
        },
    );

    assert!(did_configure_config_dir);
    assert_eq!(
        configured_plugin_dirs,
        Some((paths.plugin_dir, paths.generated_plugin_dir))
    );
    assert_eq!(configured_config_dir, paths.config_dir);
}

#[test]
fn bundled_runtime_path_configuration_skips_missing_config_dir() {
    let temp = tempdir().expect("tempdir");
    let paths = BundledRuntimePaths {
        plugin_dir: temp.path().join("Resources/plugins/app"),
        config_dir: None,
        generated_plugin_dir: temp.path().join("AppData/generated/claude-plugin"),
    };

    let configured_config_dir =
        configure_bundled_runtime_paths(paths, |_plugin_dir, _generated_plugin_dir| {}, drop);

    assert!(!configured_config_dir);
}
