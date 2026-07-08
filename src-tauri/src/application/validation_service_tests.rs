use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use super::validation_service::configure_validation_shell_command_for_test;

struct EnvGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn path_index(entries: &[PathBuf], path: impl AsRef<Path>) -> usize {
    entries
        .iter()
        .position(|entry| entry == path.as_ref())
        .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
}

#[test]
fn validation_shell_command_preserves_user_shims_while_ensuring_node_bin() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
    let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

    let mut command = tokio::process::Command::new("/usr/bin/env");
    configure_validation_shell_command_for_test(&mut command);

    let path_value = command
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
        })
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();

    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        assert!(
            path_index(&path_entries, &cargo_bin) < path_index(&path_entries, "/tmp/fake-node-bin"),
            "user cargo shim should stay before inserted Node bin: {path_value:?}"
        );
    }
    assert!(
        path_index(&path_entries, "/tmp/fake-node-bin") < path_index(&path_entries, "/usr/bin")
    );
}
