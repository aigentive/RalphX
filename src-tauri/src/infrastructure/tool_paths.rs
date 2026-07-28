use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

#[cfg(test)]
pub(crate) static TEST_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SYSTEM_FALLBACK_PATHS: &[&str] = &[
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

const USER_TOOLCHAIN_RELATIVE_PATHS: &[&str] = &[
    ".cargo/bin",
    ".local/bin",
    "bin",
    ".asdf/shims",
    ".asdf/bin",
    ".mise/shims",
    ".mise/bin",
    ".rbenv/shims",
    ".rbenv/bin",
    ".pyenv/shims",
    ".pyenv/bin",
    ".nodenv/shims",
    ".nodenv/bin",
    ".volta/bin",
];

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
    let captured_shell_path = crate::infrastructure::login_shell_env::captured_path();
    let existing_path = std::env::var_os("PATH");
    let home_dir = dirs::home_dir();
    let nvm_bin = find_env_dir("NVM_BIN");
    let volta_bin = find_env_dir("VOLTA_HOME").map(|path| path.join("bin"));

    agent_subprocess_env_path_from_parts_with_shell(
        captured_shell_path.as_deref(),
        existing_path.as_deref(),
        home_dir.as_deref(),
        nvm_bin.as_deref(),
        volta_bin.as_deref(),
    )
}

#[cfg(test)]
pub(crate) fn agent_subprocess_env_path_from_parts(
    existing_path: Option<&OsStr>,
    home_dir: Option<&Path>,
) -> OsString {
    agent_subprocess_env_path_from_parts_with_shell(None, existing_path, home_dir, None, None)
}

fn agent_subprocess_env_path_from_parts_with_shell(
    captured_shell_path: Option<&OsStr>,
    existing_path: Option<&OsStr>,
    home_dir: Option<&Path>,
    nvm_bin: Option<&Path>,
    volta_bin: Option<&Path>,
) -> OsString {
    let mut entries = Vec::new();
    let mut delayed_system_entries = Vec::new();
    let mut join_fallback = None;

    if let Some(captured_shell_path) = non_empty_path(captured_shell_path) {
        join_fallback = Some(captured_shell_path);
        entries.extend(non_empty_split_paths(captured_shell_path));
    } else if let Some(existing_path) = non_empty_path(existing_path) {
        join_fallback = Some(existing_path);
        for entry in non_empty_split_paths(existing_path) {
            if is_system_fallback_path(&entry) {
                delayed_system_entries.push(entry);
            } else {
                entries.push(entry);
            }
        }
    }

    if let Some(home_dir) = home_dir {
        entries.extend(
            USER_TOOLCHAIN_RELATIVE_PATHS
                .iter()
                .map(|relative| home_dir.join(relative)),
        );
    }

    if let Some(nvm_bin) = nvm_bin {
        entries.push(nvm_bin.to_path_buf());
    }
    if let Some(volta_bin) = volta_bin {
        entries.push(volta_bin.to_path_buf());
    }

    entries.extend(delayed_system_entries);
    entries.extend(SYSTEM_FALLBACK_PATHS.iter().map(PathBuf::from));
    dedupe_path_entries(&mut entries);

    std::env::join_paths(entries).unwrap_or_else(|_| {
        join_fallback
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| OsString::from("/usr/bin:/bin:/usr/sbin:/sbin"))
    })
}

fn non_empty_path(value: Option<&OsStr>) -> Option<&OsStr> {
    value.filter(|value| non_empty_split_paths(value).next().is_some())
}

fn non_empty_split_paths(value: &OsStr) -> impl Iterator<Item = PathBuf> + '_ {
    env::split_paths(value).filter(|entry| !entry.as_os_str().is_empty())
}

fn dedupe_path_entries(entries: &mut Vec<PathBuf>) {
    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.as_os_str().to_os_string()));
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
    let extra_candidates = javascript_tool_env_candidates("claude");
    let user_candidates = home_local_tool_candidates("claude");

    find_cli_path_with_candidate_groups(
        "claude",
        &[
            "/opt/homebrew/bin/claude",
            "/usr/local/bin/claude",
            "/usr/bin/claude",
        ],
        &extra_candidates,
        &user_candidates,
    )
    .into_iter()
    .next()
}

pub(crate) fn claude_native_cli_path() -> Option<PathBuf> {
    let home_dir = tool_path_home_dir()?;
    let candidate = home_dir
        .join(".local")
        .join("bin")
        .join(claude_native_binary_name());
    has_safe_absolute_shape(&candidate).then_some(candidate)
}

pub(crate) fn find_claude_native_cli_path() -> Option<PathBuf> {
    let candidate = claude_native_cli_path()?;
    is_launchable_file(&candidate).then_some(candidate)
}

fn claude_native_binary_name() -> &'static str {
    if cfg!(windows) {
        "claude.exe"
    } else {
        "claude"
    }
}

pub(crate) fn find_codex_cli_path() -> Option<PathBuf> {
    find_codex_cli_candidates().into_iter().next()
}

pub(crate) fn find_tailscale_cli_path() -> Option<PathBuf> {
    find_launchable_cli_path(
        "tailscale",
        &[
            "/Applications/Tailscale.app/Contents/MacOS/tailscale",
            "/opt/homebrew/bin/tailscale",
            "/usr/local/bin/tailscale",
        ],
    )
}

pub(crate) fn find_codex_cli_candidates() -> Vec<PathBuf> {
    let extra_candidates = javascript_tool_env_candidates("codex");
    let user_candidates = home_local_tool_candidates("codex");

    find_cli_path_with_candidate_groups(
        "codex",
        &[
            "/opt/homebrew/bin/codex",
            "/usr/local/bin/codex",
            "/usr/bin/codex",
        ],
        &extra_candidates,
        &user_candidates,
    )
}

pub(crate) fn find_launchable_cli_path(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
) -> Option<PathBuf> {
    let extra_candidates = javascript_tool_env_candidates(tool_name);
    let user_candidates = home_local_tool_candidates(tool_name);

    find_cli_path_with_candidate_groups(
        tool_name,
        fixed_candidates,
        &extra_candidates,
        &user_candidates,
    )
    .into_iter()
    .next()
}

pub(crate) fn find_launchable_cli_path_without_shell(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
) -> Option<PathBuf> {
    let extra_candidates = javascript_tool_env_candidates(tool_name);
    let user_candidates = home_local_tool_candidates(tool_name);

    find_cli_path_with_candidate_groups_and_shell(
        tool_name,
        fixed_candidates,
        &extra_candidates,
        &user_candidates,
        |_| None,
    )
    .into_iter()
    .next()
}

pub(crate) fn find_launchable_cli_paths_with_login_shell(
    tool_names: &[&'static str],
) -> HashMap<&'static str, PathBuf> {
    if tool_names.is_empty() || tool_names.iter().any(|name| !is_safe_tool_name(name)) {
        return HashMap::new();
    }

    let command = format!(
        "for tool in {}; do path=$(command -v \"$tool\" 2>/dev/null) && printf '%s\\t%s\\n' \"$tool\" \"$path\"; done",
        tool_names.join(" ")
    );
    let shell_started_at = Instant::now();
    let output = Command::new("/bin/zsh")
        .args(["-ilc", command.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let paths = match output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8(output.stdout).unwrap_or_default();
            launchable_cli_paths_from_shell_output(tool_names, &stdout)
        }
        _ => HashMap::new(),
    };

    tracing::info!(
        requested = tool_names.len(),
        resolved = paths.len(),
        elapsed_ms = shell_started_at.elapsed().as_millis() as u64,
        "CLI login-shell batch fallback resolution completed"
    );
    paths
}

pub(crate) fn ensure_resolved_node_bin_in_path(cmd: &mut std::process::Command) {
    let Some(node_bin_dir) = resolved_node_bin_dir() else {
        return;
    };

    let command_path = command_env_var(cmd, "PATH");
    let current_path = command_path
        .clone()
        .unwrap_or_else(agent_subprocess_env_path);
    let current_entries = non_empty_split_paths(&current_path).collect::<Vec<_>>();

    if current_entries.iter().any(|entry| entry == &node_bin_dir) {
        if command_path.is_none() {
            cmd.env("PATH", current_path);
        }
        return;
    }

    let mut paths = current_entries;
    let insertion_index = paths
        .iter()
        .position(|entry| is_system_fallback_path(entry))
        .unwrap_or(paths.len());
    paths.insert(insertion_index, node_bin_dir);
    dedupe_path_entries(&mut paths);

    if let Ok(joined) = env::join_paths(paths) {
        cmd.env("PATH", joined);
    }
}

fn is_system_fallback_path(path: &Path) -> bool {
    SYSTEM_FALLBACK_PATHS
        .iter()
        .any(|fallback| path == Path::new(fallback))
}

fn resolve_cli_path(tool_name: &'static str, fixed_candidates: &[&'static str]) -> PathBuf {
    find_cli_path(tool_name, fixed_candidates).unwrap_or_else(|| PathBuf::from(tool_name))
}

fn find_node_cli_path() -> Option<PathBuf> {
    find_env_override_path("RALPHX_NODE_PATH").or_else(|| {
        find_cli_path_with_candidates(
            "node",
            &["/opt/homebrew/bin/node", "/usr/local/bin/node"],
            &javascript_tool_env_candidates("node"),
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
    find_cli_path_candidates_with_candidates(tool_name, fixed_candidates, extra_candidates)
        .into_iter()
        .next()
}

fn find_cli_path_candidates_with_candidates(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
    extra_candidates: &[PathBuf],
) -> Vec<PathBuf> {
    find_cli_path_with_candidate_groups(tool_name, fixed_candidates, extra_candidates, &[])
}

fn find_cli_path_with_candidate_groups(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
    extra_candidates: &[PathBuf],
    user_candidates: &[PathBuf],
) -> Vec<PathBuf> {
    find_cli_path_with_candidate_groups_and_shell(
        tool_name,
        fixed_candidates,
        extra_candidates,
        user_candidates,
        find_login_shell_cli,
    )
}

fn find_cli_path_with_candidate_groups_and_shell(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
    extra_candidates: &[PathBuf],
    user_candidates: &[PathBuf],
    find_login_shell: impl FnOnce(&'static str) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(path) = which::which(tool_name) {
        if is_launchable_tool_path(tool_name, &path) {
            push_unique_path(&mut candidates, path);
        }
    }

    for candidate in extra_candidates {
        // Extra candidates are derived from trusted env-path conventions such as NVM_BIN
        // and VOLTA_HOME/bin, then validated before probing.
        // codeql[rust/path-injection]
        if is_launchable_tool_path(tool_name, candidate) {
            push_unique_path(&mut candidates, candidate.clone());
        }
    }

    for candidate in user_candidates {
        // User candidates are built from fixed tool locations under a shape-validated home dir.
        // codeql[rust/path-injection]
        if is_launchable_tool_path(tool_name, candidate) {
            push_unique_path(&mut candidates, candidate.clone());
        }
    }

    for candidate in fixed_candidates {
        let path = PathBuf::from(candidate);
        // Fixed, app-owned candidate list for GUI launches with stripped PATH.
        // codeql[rust/path-injection]
        if is_launchable_tool_path(tool_name, &path) {
            push_unique_path(&mut candidates, path);
        }
    }

    if candidates.is_empty() {
        let shell_started_at = Instant::now();
        let shell_path = find_login_shell(tool_name);
        tracing::info!(
            tool = tool_name,
            success = shell_path.is_some(),
            elapsed_ms = shell_started_at.elapsed().as_millis() as u64,
            "CLI login-shell fallback resolution completed"
        );
        if let Some(path) = shell_path {
            push_unique_path(&mut candidates, path);
        }
    }

    candidates
}

#[cfg(test)]
pub(crate) fn find_cli_path_with_candidate_groups_for_test(
    tool_name: &'static str,
    fixed_candidates: &[&'static str],
    extra_candidates: &[PathBuf],
    user_candidates: &[PathBuf],
    find_login_shell: impl FnOnce(&'static str) -> Option<PathBuf>,
) -> Vec<PathBuf> {
    find_cli_path_with_candidate_groups_and_shell(
        tool_name,
        fixed_candidates,
        extra_candidates,
        user_candidates,
        find_login_shell,
    )
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|candidate| candidate == &path) {
        paths.push(path);
    }
}

fn find_env_override_path(env_var: &'static str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(env_var).ok()?);
    has_safe_absolute_shape(&path).then_some(path)
}

fn find_env_dir(env_var: &'static str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(env_var).ok()?);
    has_safe_absolute_shape(&path).then_some(path)
}

fn javascript_tool_env_candidates(tool_name: &'static str) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Some(nvm_bin) = find_env_dir("NVM_BIN") {
        candidates.push(nvm_bin.join(tool_name));
    }
    if let Some(volta_home) = find_env_dir("VOLTA_HOME") {
        candidates.push(volta_home.join("bin").join(tool_name));
    }
    candidates.extend(nvm_versioned_tool_candidates(tool_name));

    candidates
}

fn home_local_tool_candidates(tool_name: &'static str) -> Vec<PathBuf> {
    let Some(home_dir) = tool_path_home_dir() else {
        return Vec::new();
    };

    let candidate = home_dir.join(".local").join("bin").join(tool_name);
    matches_tool_path(tool_name, &candidate)
        .then_some(candidate)
        .into_iter()
        .collect()
}

fn nvm_versioned_tool_candidates(tool_name: &'static str) -> Vec<PathBuf> {
    let Some(home_dir) = tool_path_home_dir() else {
        return Vec::new();
    };

    nvm_versioned_tool_candidates_from_home(tool_name, &home_dir)
}

#[cfg(test)]
fn tool_path_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(dirs::home_dir)
        .filter(|path| has_safe_absolute_shape(path))
}

#[cfg(not(test))]
fn tool_path_home_dir() -> Option<PathBuf> {
    dirs::home_dir().filter(|path| has_safe_absolute_shape(path))
}

fn nvm_versioned_tool_candidates_from_home(
    tool_name: &'static str,
    home_dir: &Path,
) -> Vec<PathBuf> {
    if !has_safe_absolute_shape(home_dir) {
        return Vec::new();
    }

    let versions_root = home_dir.join(".nvm").join("versions").join("node");
    if !has_safe_absolute_shape(&versions_root) {
        return Vec::new();
    }

    // Fixed NVM root under the validated home directory; each accepted child
    // directory must be a direct semver-shaped `vX.Y.Z` component.
    // codeql[rust/path-injection]
    let Ok(entries) = std::fs::read_dir(&versions_root) else {
        return Vec::new();
    };

    let mut candidates = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version_dir = entry.path();
            if !is_direct_child_of(&version_dir, &versions_root) {
                return None;
            }

            let version_name = version_dir.file_name()?.to_str()?;
            let version = parse_nvm_node_version_dir(version_name)?;
            let candidate = version_dir.join("bin").join(tool_name);
            if !matches_tool_path(tool_name, &candidate) {
                return None;
            }

            // Candidate path is built from the fixed NVM root, a semver-shaped
            // child directory, and fixed `bin/<tool>` components.
            // codeql[rust/path-injection]
            if !is_launchable_tool_path(tool_name, &candidate) {
                return None;
            }

            Some((version, candidate))
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(left_version, _), (right_version, _)| right_version.cmp(left_version));
    candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

fn parse_nvm_node_version_dir(name: &str) -> Option<(u64, u64, u64)> {
    let version = name.strip_prefix('v')?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next()?.parse::<u64>().ok()?;
    let patch = parts.next()?.parse::<u64>().ok()?;
    parts.next().is_none().then_some((major, minor, patch))
}

fn find_login_shell_cli(tool_name: &'static str) -> Option<PathBuf> {
    let command = format!("command -v {tool_name}");
    let output = Command::new("/bin/zsh")
        .args(["-ilc", command.as_str()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    launchable_cli_path_from_shell_output(tool_name, &stdout)
}

pub(crate) fn launchable_cli_path_from_shell_output(
    tool_name: &str,
    output: &str,
) -> Option<PathBuf> {
    output.lines().rev().find_map(|line| {
        let candidate = PathBuf::from(line.trim());
        is_launchable_tool_path(tool_name, &candidate).then_some(candidate)
    })
}

fn launchable_cli_paths_from_shell_output(
    tool_names: &[&'static str],
    output: &str,
) -> HashMap<&'static str, PathBuf> {
    let requested = tool_names.iter().copied().collect::<HashSet<_>>();
    let mut paths = HashMap::new();
    for line in output.lines() {
        let Some((name, path)) = line.split_once('\t') else {
            continue;
        };
        let Some(tool_name) = requested.get(name).copied() else {
            continue;
        };
        let candidate = PathBuf::from(path.trim());
        if is_launchable_tool_path(tool_name, &candidate) {
            paths.entry(tool_name).or_insert(candidate);
        }
    }
    paths
}

#[cfg(test)]
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

fn is_safe_tool_name(tool_name: &str) -> bool {
    !tool_name.is_empty()
        && tool_name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_launchable_tool_path(tool_name: &str, path: &Path) -> bool {
    matches_tool_path(tool_name, path) && is_launchable_file(path)
}

pub(crate) fn has_safe_absolute_binary_path_shape(path: &Path) -> bool {
    has_safe_absolute_shape(path)
}

pub(crate) fn is_safe_launchable_binary_path(path: &Path) -> bool {
    has_safe_absolute_shape(path) && is_launchable_file(path)
}

fn is_launchable_file(path: &Path) -> bool {
    // Callers must validate absolute path shape before this filesystem sink when
    // paths are influenced by settings or env state.
    // codeql[rust/path-injection]
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn is_direct_child_of(path: &Path, parent: &Path) -> bool {
    path.parent() == Some(parent)
        && path
            .file_name()
            .and_then(OsStr::to_str)
            .map(|name| !name.is_empty())
            .unwrap_or(false)
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
        agent_subprocess_env_path_from_parts, agent_subprocess_env_path_from_parts_with_shell,
        claude_native_cli_path, ensure_resolved_node_bin_in_path, find_claude_cli_path,
        find_claude_native_cli_path, find_cli_path_with_candidate_groups_for_test,
        find_codex_cli_candidates, find_codex_cli_path, find_launchable_cli_path,
        find_launchable_cli_path_without_shell, find_launchable_cli_paths_with_login_shell,
        find_tailscale_cli_path, is_safe_tool_name, launchable_cli_paths_from_shell_output,
        nvm_versioned_tool_candidates_from_home, parse_nvm_node_version_dir, resolve_node_cli_path,
        safe_cli_path_from_shell_output, TEST_ENV_MUTEX,
    };
    use std::cell::Cell;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};

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

    fn path_entries(path: &std::ffi::OsStr) -> Vec<PathBuf> {
        std::env::split_paths(path).collect::<Vec<_>>()
    }

    fn path_index(entries: &[PathBuf], path: impl AsRef<Path>) -> usize {
        entries
            .iter()
            .position(|entry| entry == path.as_ref())
            .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
    }

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
    fn shell_batch_output_accepts_only_requested_safe_launchable_tools() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let cursor = temp_dir.path().join("cursor");
        let code = temp_dir.path().join("code");
        write_fake_tool(&cursor);
        write_fake_tool(&code);
        let output = format!(
            "cursor\t{}\ncode\t{}\nother\t{}\n",
            cursor.display(),
            code.display(),
            temp_dir.path().join("other").display()
        );

        let paths = launchable_cli_paths_from_shell_output(&["cursor", "code"], &output);

        assert_eq!(paths.get("cursor"), Some(&cursor));
        assert_eq!(paths.get("code"), Some(&code));
        assert!(!paths.contains_key("other"));
    }

    #[test]
    fn shell_batch_output_ignores_malformed_lines() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let cursor = temp_dir.path().join("cursor");
        write_fake_tool(&cursor);
        let output = format!(
            "malformed\ncursor\t{}\ncode\t{}\n",
            cursor.display(),
            temp_dir.path().join("missing-code").display()
        );

        let paths = launchable_cli_paths_from_shell_output(&["cursor", "code"], &output);

        assert_eq!(paths.get("cursor"), Some(&cursor));
        assert!(!paths.contains_key("code"));
    }

    #[test]
    fn safe_tool_name_rejects_shell_metacharacters() {
        assert!(is_safe_tool_name("code-insiders"));
        assert!(is_safe_tool_name("explorer.exe"));
        assert!(!is_safe_tool_name("code;rm"));
        assert!(!is_safe_tool_name("bad tool"));
    }

    #[test]
    fn find_launchable_cli_path_uses_nvm_bin_candidate() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let nvm_bin = temp_dir.path().join("nvm-bin");
        std::fs::create_dir_all(&nvm_bin).expect("create nvm bin");
        let launcher = nvm_bin.join("fake-launcher");
        write_fake_tool(&launcher);

        let _path = EnvGuard::set_os("PATH", "");
        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _nvm_bin = EnvGuard::set_os("NVM_BIN", &nvm_bin);
        let _volta_home = EnvGuard::unset("VOLTA_HOME");

        assert_eq!(
            find_launchable_cli_path("fake-launcher", &[]),
            Some(launcher)
        );
    }

    #[test]
    fn find_tailscale_cli_path_uses_path_candidate() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let bin_dir = temp_dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir).expect("create bin dir");
        let tailscale = bin_dir.join("tailscale");
        write_fake_tool(&tailscale);

        let _path = EnvGuard::set_os("PATH", &bin_dir);
        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _volta_home = EnvGuard::unset("VOLTA_HOME");

        assert_eq!(find_tailscale_cli_path(), Some(tailscale));
    }

    #[test]
    fn find_launchable_cli_path_without_shell_uses_fixed_candidate() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let launcher = temp_dir.path().join("fake-launcher");
        write_fake_tool(&launcher);
        let fixed_candidate: &'static str =
            Box::leak(launcher.to_string_lossy().into_owned().into_boxed_str());

        let _path = EnvGuard::set_os("PATH", "");
        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _volta_home = EnvGuard::unset("VOLTA_HOME");

        assert_eq!(
            find_launchable_cli_path_without_shell("fake-launcher", &[fixed_candidate]),
            Some(launcher)
        );
    }

    #[test]
    fn login_shell_batch_rejects_empty_or_unsafe_tool_names() {
        assert!(find_launchable_cli_paths_with_login_shell(&[]).is_empty());
        assert!(find_launchable_cli_paths_with_login_shell(&["code;rm"]).is_empty());
    }

    #[test]
    fn login_shell_batch_returns_empty_for_missing_tool() {
        let paths = find_launchable_cli_paths_with_login_shell(&["definitely-missing-ralphx-tool"]);

        assert!(paths.is_empty());
    }

    #[test]
    fn cli_resolution_skips_login_shell_when_candidate_exists() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let candidate = temp_dir.path().join("fake-ralphx-cli");
        write_fake_tool(&candidate);
        let shell_called = Cell::new(false);

        let paths = find_cli_path_with_candidate_groups_for_test(
            "fake-ralphx-cli",
            &[],
            std::slice::from_ref(&candidate),
            &[],
            |_| {
                shell_called.set(true);
                None
            },
        );

        assert_eq!(paths, vec![candidate]);
        assert!(
            !shell_called.get(),
            "login shell fallback must be last-resort only"
        );
    }

    #[test]
    fn cli_resolution_uses_login_shell_when_no_candidate_exists() {
        let shell_candidate = PathBuf::from("/tmp/fake-ralphx-cli");
        let shell_called = Cell::new(false);

        let paths =
            find_cli_path_with_candidate_groups_for_test("fake-ralphx-cli", &[], &[], &[], |_| {
                shell_called.set(true);
                Some(shell_candidate.clone())
            });

        assert!(shell_called.get());
        assert_eq!(paths, vec![shell_candidate]);
    }

    #[test]
    fn agent_subprocess_path_preserves_existing_path_and_adds_common_dev_bins() {
        let path = agent_subprocess_env_path_from_parts(
            Some(OsStr::new("/existing/bin:/usr/bin")),
            Some(Path::new("/Users/example")),
        );
        let entries = path_entries(path.as_os_str());

        assert!(entries.contains(&PathBuf::from("/existing/bin")));
        assert!(entries.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(entries.contains(&PathBuf::from("/usr/local/bin")));
        assert!(entries.contains(&PathBuf::from("/Users/example/.cargo/bin")));
        assert!(entries.contains(&PathBuf::from("/Users/example/.asdf/shims")));
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin") < path_index(&entries, "/usr/bin")
        );
    }

    #[test]
    fn agent_subprocess_path_prefers_captured_shell_order_over_app_path() {
        let path = agent_subprocess_env_path_from_parts_with_shell(
            Some(OsStr::new(
                "/Users/example/.cargo/bin:/opt/homebrew/bin:/usr/bin",
            )),
            Some(OsStr::new(
                "/opt/homebrew/bin:/Users/example/.cargo/bin:/bin",
            )),
            Some(Path::new("/Users/example")),
            None,
            None,
        );
        let entries = path_entries(path.as_os_str());

        assert_eq!(
            entries.first(),
            Some(&PathBuf::from("/Users/example/.cargo/bin"))
        );
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
        assert_eq!(
            entries
                .iter()
                .filter(|entry| *entry == &PathBuf::from("/Users/example/.cargo/bin"))
                .count(),
            1
        );
    }

    #[test]
    fn agent_subprocess_path_adds_user_shims_before_fallbacks_when_path_is_stripped() {
        let path = agent_subprocess_env_path_from_parts_with_shell(
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            Some(Path::new("/Users/example")),
            None,
            None,
        );
        let entries = path_entries(path.as_os_str());

        assert!(entries.contains(&PathBuf::from("/Users/example/.mise/shims")));
        assert!(entries.contains(&PathBuf::from("/Users/example/.mise/bin")));
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin") < path_index(&entries, "/usr/bin")
        );
        assert!(
            path_index(&entries, "/Users/example/.mise/shims")
                < path_index(&entries, "/usr/local/bin")
        );
    }

    #[test]
    fn agent_subprocess_path_delays_app_process_system_dirs_behind_user_shims() {
        let path = agent_subprocess_env_path_from_parts_with_shell(
            None,
            Some(OsStr::new("/opt/homebrew/bin:/custom/bin:/usr/bin")),
            Some(Path::new("/Users/example")),
            None,
            None,
        );
        let entries = path_entries(path.as_os_str());

        assert_eq!(entries.first(), Some(&PathBuf::from("/custom/bin")));
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin") < path_index(&entries, "/usr/bin")
        );
    }

    #[test]
    fn agent_subprocess_path_places_node_env_dirs_before_system_fallbacks() {
        let path = agent_subprocess_env_path_from_parts_with_shell(
            None,
            Some(OsStr::new("/usr/bin:/bin")),
            Some(Path::new("/Users/example")),
            Some(Path::new("/Users/example/.nvm/current/bin")),
            Some(Path::new("/Users/example/.volta/bin")),
        );
        let entries = path_entries(path.as_os_str());

        assert!(
            path_index(&entries, "/Users/example/.nvm/current/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
        assert!(
            path_index(&entries, "/Users/example/.volta/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
    }

    #[test]
    fn agent_subprocess_path_resolves_fake_rustc_from_user_shim_before_homebrew() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let home = temp_dir.path().join("home");
        let cargo_bin = home.join(".cargo").join("bin");
        let homebrew_bin = temp_dir.path().join("homebrew").join("bin");
        std::fs::create_dir_all(&cargo_bin).expect("create cargo bin");
        std::fs::create_dir_all(&homebrew_bin).expect("create homebrew bin");
        write_fake_tool(&cargo_bin.join("rustc"));
        write_fake_tool(&homebrew_bin.join("rustc"));

        let existing_path = std::env::join_paths([homebrew_bin.as_path()]).expect("join path");
        let path = agent_subprocess_env_path_from_parts_with_shell(
            Some(cargo_bin.as_os_str()),
            Some(existing_path.as_os_str()),
            Some(&home),
            None,
            None,
        );
        let resolved = path_entries(path.as_os_str())
            .into_iter()
            .map(|entry| entry.join("rustc"))
            .find(|candidate| candidate.exists())
            .expect("fake rustc should resolve");

        assert_eq!(resolved, cargo_bin.join("rustc"));
    }

    #[test]
    fn resolve_node_cli_path_uses_nvm_bin_when_path_is_stripped() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let nvm_bin = temp_dir.path().join("nvm-bin");
        std::fs::create_dir_all(&nvm_bin).expect("create nvm bin");
        write_fake_tool(&nvm_bin.join("node"));

        let _path = EnvGuard::set_os("PATH", "");
        let _nvm_bin = EnvGuard::set_os("NVM_BIN", &nvm_bin);
        let _volta_home = EnvGuard::unset("VOLTA_HOME");
        let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

        assert_eq!(resolve_node_cli_path(), nvm_bin.join("node"));
    }

    #[test]
    fn resolve_node_cli_path_uses_volta_home_when_path_is_stripped() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let volta_home = temp_dir.path().join("volta-home");
        let volta_bin = volta_home.join("bin");
        std::fs::create_dir_all(&volta_bin).expect("create volta bin");
        write_fake_tool(&volta_bin.join("node"));

        let _path = EnvGuard::set_os("PATH", "");
        let _volta_home = EnvGuard::set_os("VOLTA_HOME", &volta_home);
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

        assert_eq!(resolve_node_cli_path(), volta_bin.join("node"));
    }

    #[test]
    fn resolve_node_cli_path_uses_nvm_versioned_bin_when_path_is_stripped() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let nvm_node_bin = temp_dir
            .path()
            .join(".nvm")
            .join("versions")
            .join("node")
            .join("v22.16.0")
            .join("bin");
        std::fs::create_dir_all(&nvm_node_bin).expect("create nvm node bin");
        write_fake_tool(&nvm_node_bin.join("node"));

        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _path = EnvGuard::set_os("PATH", "");
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _volta_home = EnvGuard::unset("VOLTA_HOME");
        let _node_override = EnvGuard::unset("RALPHX_NODE_PATH");

        assert_eq!(resolve_node_cli_path(), nvm_node_bin.join("node"));
    }

    #[test]
    fn find_codex_cli_path_uses_nvm_versioned_bin_when_path_is_stripped() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let older_codex_bin = temp_dir
            .path()
            .join(".nvm")
            .join("versions")
            .join("node")
            .join("v20.9.0")
            .join("bin");
        let newer_codex_bin = temp_dir
            .path()
            .join(".nvm")
            .join("versions")
            .join("node")
            .join("v22.16.0")
            .join("bin");
        std::fs::create_dir_all(&older_codex_bin).expect("create older nvm bin");
        std::fs::create_dir_all(&newer_codex_bin).expect("create newer nvm bin");
        write_fake_tool(&older_codex_bin.join("codex"));
        write_fake_tool(&newer_codex_bin.join("codex"));

        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _path = EnvGuard::set_os("PATH", "");
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _volta_home = EnvGuard::unset("VOLTA_HOME");

        assert_eq!(find_codex_cli_path(), Some(newer_codex_bin.join("codex")));
        assert_eq!(
            find_codex_cli_candidates()
                .into_iter()
                .take(2)
                .collect::<Vec<_>>(),
            vec![newer_codex_bin.join("codex"), older_codex_bin.join("codex")]
        );
    }

    #[test]
    fn find_claude_cli_path_uses_nvm_versioned_bin_when_path_is_stripped() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let claude_bin = temp_dir
            .path()
            .join(".nvm")
            .join("versions")
            .join("node")
            .join("v22.16.0")
            .join("bin");
        std::fs::create_dir_all(&claude_bin).expect("create nvm bin");
        write_fake_tool(&claude_bin.join("claude"));

        let _home = EnvGuard::set_os("HOME", temp_dir.path());
        let _path = EnvGuard::set_os("PATH", "");
        let _nvm_bin = EnvGuard::unset("NVM_BIN");
        let _volta_home = EnvGuard::unset("VOLTA_HOME");

        assert_eq!(find_claude_cli_path(), Some(claude_bin.join("claude")));
    }

    #[test]
    fn claude_native_cli_path_uses_documented_home_local_location() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let native_path = temp_dir
            .path()
            .join(".local")
            .join("bin")
            .join(if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            });
        std::fs::create_dir_all(native_path.parent().unwrap()).expect("create native bin");

        let _home = EnvGuard::set_os("HOME", temp_dir.path());

        assert_eq!(claude_native_cli_path(), Some(native_path.clone()));
        assert_eq!(find_claude_native_cli_path(), None);

        write_fake_tool(&native_path);

        assert_eq!(find_claude_native_cli_path(), Some(native_path));
    }

    #[test]
    fn nvm_versioned_tool_candidates_skip_non_executable_newer_tool() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let versions_root = temp_dir.path().join(".nvm").join("versions").join("node");
        let older_bin = versions_root.join("v20.9.0").join("bin");
        let newer_bin = versions_root.join("v22.16.0").join("bin");
        std::fs::create_dir_all(&older_bin).expect("create older nvm bin");
        std::fs::create_dir_all(&newer_bin).expect("create newer nvm bin");
        write_fake_tool(&older_bin.join("codex"));
        std::fs::write(newer_bin.join("codex"), "").expect("write non-executable codex");

        assert_eq!(
            nvm_versioned_tool_candidates_from_home("codex", temp_dir.path()),
            vec![older_bin.join("codex")]
        );
    }

    #[test]
    fn nvm_versioned_tool_candidates_rejects_non_semver_dirs() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let versions_root = temp_dir.path().join(".nvm").join("versions").join("node");
        let valid_bin = versions_root.join("v20.10.0").join("bin");
        let invalid_bin = versions_root.join("latest").join("bin");
        std::fs::create_dir_all(&valid_bin).expect("create valid nvm bin");
        std::fs::create_dir_all(&invalid_bin).expect("create invalid nvm bin");
        write_fake_tool(&valid_bin.join("codex"));
        write_fake_tool(&invalid_bin.join("codex"));

        assert_eq!(
            nvm_versioned_tool_candidates_from_home("codex", temp_dir.path()),
            vec![valid_bin.join("codex")]
        );
    }

    #[test]
    fn parse_nvm_node_version_dir_requires_three_numeric_components() {
        assert_eq!(parse_nvm_node_version_dir("v22.16.0"), Some((22, 16, 0)));
        assert_eq!(parse_nvm_node_version_dir("22.16.0"), None);
        assert_eq!(parse_nvm_node_version_dir("v22.16"), None);
        assert_eq!(parse_nvm_node_version_dir("v22.x.0"), None);
    }

    #[test]
    fn ensure_resolved_node_bin_in_path_preserves_user_shim_precedence() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/opt/homebrew/bin/node");
        let mut cmd = std::process::Command::new("/usr/bin/env");
        cmd.env("PATH", "/Users/example/.cargo/bin:/usr/bin:/bin");

        ensure_resolved_node_bin_in_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        let entries = path_entries(&path_value);

        assert_eq!(
            entries.first(),
            Some(&PathBuf::from("/Users/example/.cargo/bin"))
        );
        assert!(
            path_index(&entries, "/Users/example/.cargo/bin")
                < path_index(&entries, "/opt/homebrew/bin")
        );
    }

    #[test]
    fn ensure_resolved_node_bin_in_path_is_noop_when_node_bin_already_exists() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
        let mut cmd = std::process::Command::new("/usr/bin/env");
        cmd.env(
            "PATH",
            "/Users/example/.cargo/bin:/tmp/fake-node-bin:/usr/bin:/bin",
        );

        ensure_resolved_node_bin_in_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        assert_eq!(
            path_entries(&path_value),
            vec![
                PathBuf::from("/Users/example/.cargo/bin"),
                PathBuf::from("/tmp/fake-node-bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }

    #[test]
    fn ensure_resolved_node_bin_in_path_uses_shared_policy_when_command_path_is_unset() {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
        let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
        let mut cmd = std::process::Command::new("/usr/bin/env");

        ensure_resolved_node_bin_in_path(&mut cmd);

        let path_value = cmd
            .get_envs()
            .find_map(|(key, value)| {
                (key == OsStr::new("PATH")).then(|| value.map(|v| v.to_os_string()))?
            })
            .expect("PATH env");
        let entries = path_entries(&path_value);

        assert!(entries.contains(&PathBuf::from("/tmp/fake-node-bin")));
        assert!(path_index(&entries, "/tmp/fake-node-bin") < path_index(&entries, "/usr/bin"));
        assert!(
            entries.iter().any(|entry| entry.ends_with(".cargo/bin")),
            "shared policy should add user toolchain shims"
        );
    }
}
