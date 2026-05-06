use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn resolve_gh_cli_path() -> PathBuf {
    resolve_cli_path("gh", &["/opt/homebrew/bin/gh", "/usr/local/bin/gh"])
}

pub(crate) fn resolve_git_cli_path() -> PathBuf {
    resolve_cli_path(
        "git",
        &[
            "/opt/homebrew/bin/git",
            "/usr/local/bin/git",
            "/usr/bin/git",
        ],
    )
}

pub(crate) fn resolve_node_cli_path() -> PathBuf {
    find_node_cli_path().unwrap_or_else(|| PathBuf::from("node"))
}

pub(crate) fn resolve_shell_cli_path() -> PathBuf {
    resolve_cli_path("sh", &["/bin/sh", "/usr/bin/sh"])
}

pub(crate) fn resolve_rm_cli_path() -> PathBuf {
    resolve_cli_path("rm", &["/bin/rm", "/usr/bin/rm"])
}

pub(crate) fn resolve_ps_cli_path() -> PathBuf {
    resolve_cli_path("ps", &["/bin/ps", "/usr/bin/ps"])
}

pub(crate) fn resolve_lsof_cli_path() -> PathBuf {
    resolve_cli_path("lsof", &["/usr/sbin/lsof", "/usr/bin/lsof"])
}

pub(crate) fn resolve_pgrep_cli_path() -> PathBuf {
    resolve_cli_path("pgrep", &["/usr/bin/pgrep"])
}

pub(crate) fn resolve_pkill_cli_path() -> PathBuf {
    resolve_cli_path("pkill", &["/usr/bin/pkill"])
}

pub(crate) fn agent_subprocess_env_path() -> OsString {
    agent_subprocess_env_path_from_parts(
        std::env::var_os("PATH").as_deref(),
        dirs::home_dir().as_deref(),
    )
}

pub(crate) fn agent_subprocess_env_path_from_parts(
    existing_path: Option<&OsStr>,
    home_dir: Option<&Path>,
) -> OsString {
    let mut entries = Vec::new();
    if let Some(existing_path) = existing_path {
        entries.extend(std::env::split_paths(existing_path));
    }

    entries.extend(
        [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
        ]
        .into_iter()
        .map(PathBuf::from),
    );

    if let Some(home_dir) = home_dir {
        entries.extend(
            [
                ".local/bin",
                "bin",
                ".cargo/bin",
                ".rbenv/bin",
                ".rbenv/shims",
                ".asdf/bin",
                ".asdf/shims",
                ".pyenv/bin",
                ".pyenv/shims",
                ".nodenv/bin",
                ".nodenv/shims",
                ".volta/bin",
            ]
            .into_iter()
            .map(|relative| home_dir.join(relative)),
        );
    }

    let mut seen = std::collections::HashSet::new();
    entries.retain(|entry| seen.insert(entry.as_os_str().to_os_string()));
    std::env::join_paths(entries).unwrap_or_else(|_| {
        existing_path
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"))
    })
}

#[cfg(windows)]
pub(crate) fn resolve_taskkill_cli_path() -> PathBuf {
    resolve_cli_path("taskkill", &[])
}

#[cfg(windows)]
pub(crate) fn resolve_tasklist_cli_path() -> PathBuf {
    resolve_cli_path("tasklist", &[])
}

pub(crate) fn find_claude_cli_path() -> Option<PathBuf> {
    find_cli_path(
        "claude",
        &[
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
            "/usr/bin/claude",
        ],
    )
}

pub(crate) fn find_codex_cli_path() -> Option<PathBuf> {
    find_cli_path(
        "codex",
        &[
            "/opt/homebrew/bin/codex",
            "/usr/local/bin/codex",
            "/usr/bin/codex",
        ],
    )
}

pub(crate) fn prepend_resolved_node_bin_to_path(cmd: &mut std::process::Command) {
    let Some(node_bin_dir) = resolved_node_bin_dir() else {
        return;
    };

    let current_path = command_env_var(cmd, "PATH").or_else(|| env::var_os("PATH"));
    let already_first = current_path
        .as_ref()
        .and_then(|value| env::split_paths(value).next())
        .map(|value| value == node_bin_dir)
        .unwrap_or(false);
    if already_first {
        return;
    }

    let mut paths = vec![node_bin_dir];
    if let Some(existing) = current_path.as_ref() {
        paths.extend(env::split_paths(existing));
    }

    if let Ok(joined) = env::join_paths(paths) {
        cmd.env("PATH", joined);
    }
}

fn resolve_cli_path(tool_name: &'static str, fixed_candidates: &[&'static str]) -> PathBuf {
    find_cli_path(tool_name, fixed_candidates).unwrap_or_else(|| PathBuf::from(tool_name))
}

fn find_node_cli_path() -> Option<PathBuf> {
    find_env_override_path("RALPHX_NODE_PATH").or_else(|| {
        find_cli_path_with_candidates(
            "node",
            &["/opt/homebrew/bin/node", "/usr/local/bin/node"],
            &node_env_candidates(),
        )
    })
}

fn find_cli_path(tool_name: &'static str, fixed_candidates: &[&'static str]) -> Option<PathBuf> {
    find_cli_path_with_candidates(tool_name, fixed_candidates, &[])
}

fn find_cli_path_with_candidates(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
    extra_candidates: &[PathBuf],
) -> Option<PathBuf> {
    if let Ok(path) = which::which(tool_name) {
        if matches_tool_path(tool_name, &path) {
            return Some(path);
        }
    }

    for candidate in extra_candidates {
        // Extra candidates are derived from trusted env-path conventions such as NVM_BIN
        // and VOLTA_HOME/bin, then validated before probing.
        // codeql[rust/path-injection]
        if matches_tool_path(tool_name, candidate) && candidate.exists() {
            return Some(candidate.clone());
        }
    }

    for candidate in fixed_candidates {
        let path = PathBuf::from(candidate);
        // Fixed, app-owned candidate list for GUI launches with stripped PATH.
        // codeql[rust/path-injection]
        if matches_tool_path(tool_name, &path) && path.exists() {
            return Some(path);
        }
    }

    find_login_shell_cli(tool_name)
}

fn find_env_override_path(env_var: &'static str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(env_var).ok()?);
    has_safe_absolute_shape(&path).then_some(path)
}

fn find_env_dir(env_var: &'static str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(env_var).ok()?);
    has_safe_absolute_shape(&path).then_some(path)
}

fn node_env_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(nvm_bin) = find_env_dir("NVM_BIN") {
        candidates.push(nvm_bin.join("node"));
    }
    if let Some(volta_home) = find_env_dir("VOLTA_HOME") {
        candidates.push(volta_home.join("bin").join("node"));
    }

    candidates
}

fn find_login_shell_cli(tool_name: &'static str) -> Option<PathBuf> {
    let command = format!("command -v {tool_name}");
    let output = Command::new("/bin/zsh")
        .args(["-lc", command.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    safe_cli_path_from_shell_output(tool_name, &stdout)
}

fn safe_cli_path_from_shell_output(tool_name: &str, output: &str) -> Option<PathBuf> {
    output.lines().rev().find_map(|line| {
        let candidate = PathBuf::from(line.trim());
        if has_safe_absolute_shape(&candidate)
            && candidate.file_name() == Some(OsStr::new(tool_name))
        {
            Some(candidate)
        } else {
            None
        }
    })
}

fn command_env_var(cmd: &std::process::Command, key: &str) -> Option<OsString> {
    cmd.get_envs().find_map(|(env_key, env_value)| {
        (env_key == OsStr::new(key)).then(|| env_value.map(OsString::from))?
    })
}

fn matches_tool_path(tool_name: &str, path: &Path) -> bool {
    has_safe_absolute_shape(path) && path.file_name() == Some(OsStr::new(tool_name))
}

fn resolved_node_bin_dir() -> Option<PathBuf> {
    find_node_cli_path()?.parent().map(Path::to_path_buf)
}

fn has_safe_absolute_shape(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }

    let mut normal_components = 0usize;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {}
            Component::Normal(_) => normal_components += 1,
            Component::ParentDir | Component::CurDir => return false,
        }
    }

    normal_components > 0
}

#[cfg(test)]
mod tests {
    use super::{
        agent_subprocess_env_path_from_parts, prepend_resolved_node_bin_to_path,
        resolve_node_cli_path, safe_cli_path_from_shell_output,
    };
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

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

    #[test]
    fn shell_output_accepts_safe_absolute_matching_tool_path() {
        let path = safe_cli_path_from_shell_output(
            "claude",
            "startup noise\n/Users/example/.local/bin/claude\n",
        );

        assert_eq!(
            path.as_deref().and_then(|value| value.to_str()),
            Some("/Users/example/.local/bin/claude")
        );
    }

    #[test]
    fn shell_output_rejects_relative_or_mismatched_paths() {
        assert!(safe_cli_path_from_shell_output("claude", "../bin/claude").is_none());
        assert!(safe_cli_path_from_shell_output("claude", "/tmp/codex").is_none());
    }

    #[test]
    fn agent_subprocess_path_preserves_existing_path_and_adds_common_dev_bins() {
        let path = agent_subprocess_env_path_from_parts(
            Some(OsStr::new("/existing/bin:/usr/bin")),
            Some(Path::new("/Users/example")),
        );
        let path = path.to_string_lossy();

        assert!(path.contains("/existing/bin"));
        assert!(path.contains("/opt/homebrew/bin"));
        assert!(path.contains("/usr/local/bin"));
        assert!(path.contains("/Users/example/.cargo/bin"));
        assert!(path.contains("/Users/example/.asdf/shims"));
    }

    #[test]
    fn resolve_node_cli_path_uses_nvm_bin_when_path_is_stripped() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let nvm_bin = temp_dir.path().join("nvm-bin");
        std::fs::create_dir_all(&nvm_bin).expect("create nvm bin");
        std::fs::write(nvm_bin.join("node"), "").expect("write fake node");

        let _path = EnvGuard::set_os("PATH", "");
        let _nvm_bin = EnvGuard::set_os("NVM_BIN", &nvm_bin);
        let _volta_home = EnvGuard::unset("VOLTA_HOME");
        let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

        assert_eq!(resolve_node_cli_path(), nvm_bin.join("node"));
    }

    #[test]
    fn resolve_node_cli_path_uses_volta_home_when_path_is_stripped() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let volta_home = temp_dir.path().join("volta-home");
        let volta_bin = volta_home.join("bin");
        std::fs::create_dir_all(&volta_bin).expect("create volta bin");
        std::fs::write(volta_bin.join("node"), "").expect("write fake node");

        let _path = EnvGuard::set_os("PATH", "");
        let _volta_home = EnvGuard::set_os("VOLTA_HOME", &volta_home);
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

        assert_eq!(resolve_node_cli_path(), volta_bin.join("node"));
    }

    #[test]
    fn prepend_resolved_node_bin_to_path_preserves_existing_path() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
        let mut cmd = std::process::Command::new("/usr/bin/env");
        cmd.env("PATH", "/usr/bin:/bin");

        prepend_resolved_node_bin_to_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        assert_eq!(
            PathBuf::from("/tmp/fake-node-bin"),
            std::env::split_paths(&path_value)
                .next()
                .expect("first PATH entry")
        );
        assert_eq!(
            std::env::split_paths(&path_value).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/tmp/fake-node-bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }
    #[test]
    fn prepend_resolved_node_bin_to_path_is_noop_when_node_bin_is_already_first() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
        let mut cmd = std::process::Command::new("/usr/bin/env");
        cmd.env("PATH", "/tmp/fake-node-bin:/usr/bin:/bin");

        prepend_resolved_node_bin_to_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        assert_eq!(
            std::env::split_paths(&path_value).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/tmp/fake-node-bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }

    #[test]
    fn prepend_resolved_node_bin_to_path_uses_inherited_path_when_command_path_is_unset() {
        let _lock = ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
        let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
        let mut cmd = std::process::Command::new("/usr/bin/env");

        prepend_resolved_node_bin_to_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        assert_eq!(
            std::env::split_paths(&path_value).collect::<Vec<_>>(),
            vec![
                PathBuf::from("/tmp/fake-node-bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }
}
