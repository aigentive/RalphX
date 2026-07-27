use super::tool_paths::{
    find_claude_cli_path, find_cli_path_with_candidate_groups_for_test, find_codex_cli_path,
    find_launchable_cli_path_without_shell, launchable_cli_path_from_shell_output, TEST_ENV_MUTEX,
};
use std::ffi::OsStr;
use std::path::Path;

struct EnvGuard {
    key: &'static str,
    original: Option<std::ffi::OsString>,
}

impl EnvGuard {
    fn set_os(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let original = std::env::var_os(key);
        std::env::set_var(key, value);
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        std::env::remove_var(key);
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

fn write_fake_tool(path: &Path) {
    std::fs::write(path, "").expect("write fake tool");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake tool metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark fake tool executable");
    }
}

#[test]
fn find_claude_cli_path_uses_home_local_bin_when_path_is_stripped() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let local_bin = temp_dir.path().join(".local").join("bin");
    std::fs::create_dir_all(&local_bin).expect("create local bin");
    write_fake_tool(&local_bin.join("claude"));

    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    assert_eq!(find_claude_cli_path(), Some(local_bin.join("claude")));
}

#[test]
fn find_codex_cli_path_uses_home_local_bin_when_path_is_stripped() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let local_bin = temp_dir.path().join(".local").join("bin");
    std::fs::create_dir_all(&local_bin).expect("create local bin");
    write_fake_tool(&local_bin.join("codex"));

    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    assert_eq!(find_codex_cli_path(), Some(local_bin.join("codex")));
}

#[test]
fn tailscale_fixed_candidate_is_found_without_shell_fallback() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let app_binary = temp_dir
        .path()
        .join("Applications/Tailscale.app/Contents/MacOS/tailscale");
    std::fs::create_dir_all(app_binary.parent().expect("app binary parent"))
        .expect("create app binary parent");
    write_fake_tool(&app_binary);
    let fixed_candidate: &'static str =
        Box::leak(app_binary.to_string_lossy().into_owned().into_boxed_str());

    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", "");
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    assert_eq!(
        find_launchable_cli_path_without_shell("tailscale", &[fixed_candidate]),
        Some(app_binary)
    );
}

#[test]
fn tailscale_resolution_returns_none_when_all_candidates_are_absent() {
    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let missing = temp_dir.path().join("missing/tailscale");
    let fixed_candidate: &'static str =
        Box::leak(missing.to_string_lossy().into_owned().into_boxed_str());
    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", "");
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    let resolved = find_cli_path_with_candidate_groups_for_test(
        "tailscale",
        &[fixed_candidate],
        &[],
        &[],
        |_| None,
    );

    assert!(resolved.is_empty());
}

#[test]
fn find_claude_cli_path_uses_interactive_zshrc_when_path_is_stripped() {
    if !Path::new("/bin/zsh").exists() {
        return;
    }

    let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let custom_bin = temp_dir.path().join("custom-bin");
    std::fs::create_dir_all(&custom_bin).expect("create custom bin");
    write_fake_tool(&custom_bin.join("claude"));
    std::fs::write(
        temp_dir.path().join(".zshrc"),
        format!(
            "echo startup noise\nexport PATH=\"{}:$PATH\"\n",
            custom_bin.display()
        ),
    )
    .expect("write zshrc");

    let _home = EnvGuard::set_os("HOME", temp_dir.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp_dir.path());
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    let _nvm_bin = EnvGuard::unset("NVM_BIN");
    let _volta_home = EnvGuard::unset("VOLTA_HOME");

    assert_eq!(find_claude_cli_path(), Some(custom_bin.join("claude")));
}

#[cfg(unix)]
#[test]
fn shell_output_launchable_cli_path_requires_executable_tool() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let candidate = temp_dir.path().join("claude");
    std::fs::write(&candidate, "").expect("write non-executable tool");
    let output = format!("startup noise\n{}\n", candidate.display());

    assert!(launchable_cli_path_from_shell_output("claude", &output).is_none());

    write_fake_tool(&candidate);

    assert_eq!(
        launchable_cli_path_from_shell_output("claude", &output),
        Some(candidate)
    );
}

#[cfg(unix)]
#[test]
fn shell_output_launchable_cli_path_skips_later_non_executable_path() {
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let executable = temp_dir.path().join("bin").join("claude");
    let non_executable = temp_dir.path().join("other").join("claude");
    std::fs::create_dir_all(executable.parent().expect("executable parent"))
        .expect("create executable parent");
    std::fs::create_dir_all(non_executable.parent().expect("non-executable parent"))
        .expect("create non-executable parent");
    write_fake_tool(&executable);
    std::fs::write(&non_executable, "").expect("write non-executable tool");
    let output = format!("{}\n{}\n", executable.display(), non_executable.display());

    assert_eq!(
        launchable_cli_path_from_shell_output("claude", &output),
        Some(executable)
    );
}
