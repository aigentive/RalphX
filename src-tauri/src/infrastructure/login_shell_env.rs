//! Capture the user's login-shell environment for spawned provider CLIs.
//!
//! ## Why this exists
//!
//! macOS GUI apps (Tauri bundle launched from Finder, Spotlight, Dock) inherit
//! a stripped environment from `launchd` — no exports from `~/.zshrc`,
//! `~/.zprofile`, `~/.bash_profile`, etc. Provider CLIs spawned by RalphX
//! (`claude`, `codex`) read credentials from env vars (`ANTHROPIC_API_KEY`,
//! `OPENAI_API_KEY`, ...) and credential files under `$HOME` that the user's
//! shell profile sets up.
//!
//! In a normal terminal session the shell sources rc files before the CLI
//! runs, so the CLI sees the full env. When the same CLI is spawned from a
//! Finder-launched Tauri app, the shell never runs, the user's exports are
//! missing, and the CLI reports as unauthenticated.
//!
//! ## What this module does
//!
//! At first use, run the user's login shell once with `-ilc env` to capture
//! the env that a fresh terminal session would have, parse it, cache it, and
//! make it available to agent-spawn sites. Spawn helpers merge this map into
//! the child env BEFORE applying their own RalphX-specific overrides, so
//! values RalphX manages (`PATH`, `CLAUDE_CODE_*`, `RALPHX_*`,
//! `TAURI_API_URL`, ...) still win on conflict.
//!
//! Designed to be **best-effort and non-blocking-fatal**: if the shell probe
//! fails for any reason, the cache is populated with an empty map and the
//! rest of the system runs as before. No panic, no crash.

use std::collections::HashMap;
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::OnceLock;

/// Cached login-shell env, populated lazily on first call to [`captured`].
static CACHE: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();

/// Test-only override. When set via [`set_for_test`], short-circuits the
/// shell probe and uses the supplied map instead.
#[cfg(test)]
static TEST_OVERRIDE: OnceLock<Arc<HashMap<String, String>>> = OnceLock::new();

/// Env var that, when set to any non-empty value, suppresses the live shell
/// probe and returns an empty map. Used by tests and by callers that want to
/// opt out of the behavior (e.g., when running under unusual init systems).
pub const DISABLE_ENV_VAR: &str = "RALPHX_DISABLE_LOGIN_SHELL_ENV";

/// Keys RalphX explicitly manages on the child env. These are skipped from
/// the captured map so the spawn helpers' overrides remain authoritative.
const MANAGED_KEYS: &[&str] = &[
    "PATH",
    "RUSTC",
    "RUSTUP_TOOLCHAIN",
    "TAURI_API_URL",
    "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
    "CLAUDE_CODE_ENABLE_TASKS",
    "CLAUDE_PLUGIN_ROOT",
    "DEBUG",
];

/// Key prefixes that identify shell-internal or RalphX-internal vars that
/// should not be propagated to spawned CLIs.
const MANAGED_PREFIXES: &[&str] = &["RALPHX_"];

/// Shell-state vars we never want to propagate verbatim. They are set by the
/// shell each time it starts and tend to drift between contexts.
const SHELL_STATE_KEYS: &[&str] = &["_", "SHLVL", "OLDPWD", "PWD"];

/// Return the cached login-shell env, populating it on first call.
pub fn captured() -> Arc<HashMap<String, String>> {
    #[cfg(test)]
    if let Some(map) = TEST_OVERRIDE.get() {
        return Arc::clone(map);
    }
    Arc::clone(CACHE.get_or_init(|| Arc::new(probe_shell_env())))
}

/// Return the captured login-shell PATH without forwarding it wholesale through
/// [`apply_to_std`]. The shared subprocess PATH builder consumes this ordering.
pub(crate) fn captured_path() -> Option<OsString> {
    captured_path_from_map(&captured())
}

fn captured_path_from_map(env: &HashMap<String, String>) -> Option<OsString> {
    env.get("PATH").map(OsString::from)
}

/// Apply the captured login-shell env to the supplied tokio [`Command`] in a
/// way that does not clobber RalphX-managed keys. Call this BEFORE adding
/// the RalphX-specific overrides (e.g., `PATH`, `CLAUDE_CODE_*`) so those
/// stay authoritative on conflict.
pub fn apply_to(cmd: &mut tokio::process::Command) {
    apply_to_std(cmd.as_std_mut());
}

/// Sync variant of [`apply_to`] for [`std::process::Command`].
pub fn apply_to_std(cmd: &mut std::process::Command) {
    let captured = captured();
    for (key, value) in captured.iter() {
        if should_forward(key) {
            cmd.env(key, value);
        }
    }
    crate::infrastructure::subprocess_env_policy::github_cli_env_policy().apply_to_std_command(cmd);
}

/// Decide whether a captured env key should land on the spawned child.
fn should_forward(key: &str) -> bool {
    should_forward_with_policy(
        key,
        crate::infrastructure::subprocess_env_policy::github_cli_env_policy(),
    )
}

fn should_forward_with_policy(
    key: &str,
    policy: crate::infrastructure::subprocess_env_policy::ProviderCredentialEnvPolicy,
) -> bool {
    if key.is_empty() {
        return false;
    }
    if policy.blocks_env_key(key) {
        return false;
    }
    if MANAGED_KEYS.contains(&key) {
        return false;
    }
    if SHELL_STATE_KEYS.contains(&key) {
        return false;
    }
    if MANAGED_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
    {
        return false;
    }
    true
}

/// Probe the user's login shell for env. Returns an empty map if anything
/// goes wrong (missing shell binary, non-zero exit, timeout, parse failure).
fn probe_shell_env() -> HashMap<String, String> {
    if disabled_by_env() {
        return HashMap::new();
    }
    let Some(shell) = resolve_login_shell() else {
        return HashMap::new();
    };
    let output = std::process::Command::new(&shell)
        .args(["-ilc", "env"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => match String::from_utf8(out.stdout) {
            Ok(text) => parse_env_dump(&text),
            Err(_) => HashMap::new(),
        },
        _ => HashMap::new(),
    }
}

fn disabled_by_env() -> bool {
    matches!(std::env::var_os(DISABLE_ENV_VAR), Some(v) if !v.is_empty())
}

fn resolve_login_shell() -> Option<OsString> {
    // Honor user `$SHELL` first, falling back to the platform-typical login shell.
    if let Some(shell) = std::env::var_os("SHELL") {
        if !shell.is_empty() {
            return Some(shell);
        }
    }
    if cfg!(target_os = "macos") {
        Some(OsString::from("/bin/zsh"))
    } else if cfg!(unix) {
        Some(OsString::from("/bin/bash"))
    } else {
        None
    }
}

/// Parse the textual output of `env` into a key/value map. The format is
/// `KEY=VALUE\n` lines, where VALUE may contain `=` characters (only the
/// first `=` is the separator) and may not include literal newlines.
///
/// Lines that do not contain `=`, lines with empty keys, and lines that
/// begin with `BASH_FUNC_` (function exports) are dropped.
pub(crate) fn parse_env_dump(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::with_capacity(64);
    for line in text.lines() {
        let Some(eq) = line.find('=') else { continue };
        let (key, value) = line.split_at(eq);
        let value = &value[1..];
        if key.is_empty() || key.starts_with("BASH_FUNC_") {
            continue;
        }
        out.insert(key.to_string(), value.to_string());
    }
    out
}

/// Test-only: install an override map so tests don't shell out.
///
/// Panics if called more than once with different maps (OnceLock semantics).
#[cfg(test)]
pub(crate) fn set_for_test(map: HashMap<String, String>) {
    let _ = TEST_OVERRIDE.set(Arc::new(map));
}

#[cfg(test)]
pub(crate) fn managed_keys_for_test() -> &'static [&'static str] {
    MANAGED_KEYS
}

#[cfg(test)]
pub(crate) fn captured_path_from_map_for_test(env: &HashMap<String, String>) -> Option<OsString> {
    captured_path_from_map(env)
}

#[cfg(test)]
pub(crate) fn should_forward_for_test(key: &str) -> bool {
    should_forward(key)
}

#[cfg(test)]
pub(crate) fn should_forward_with_github_token_removal_for_test(key: &str, enabled: bool) -> bool {
    should_forward_with_policy(
        key,
        crate::infrastructure::subprocess_env_policy::ProviderCredentialEnvPolicy::github_cli_with_token_removal(enabled),
    )
}

#[cfg(test)]
pub(crate) fn disabled_by_env_for_test() -> bool {
    disabled_by_env()
}

#[cfg(test)]
pub(crate) fn resolve_login_shell_for_test() -> Option<OsString> {
    resolve_login_shell()
}

#[cfg(test)]
pub(crate) fn probe_shell_env_for_test() -> HashMap<String, String> {
    probe_shell_env()
}
