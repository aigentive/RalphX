use super::harness_runtime_registry::{
    clear_harness_runtime_caches_for_tests, probe_supported_harnesses,
    refresh_harness_runtime_probe, refresh_supported_harnesses, HarnessRuntimeProbe,
};
use crate::domain::agents::AgentHarnessKind;
use std::collections::HashMap;
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

#[cfg(unix)]
fn write_fake_executable(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake executable");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("fake executable metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark fake executable");
}

#[cfg(unix)]
fn write_fake_claude(path: &Path, version: &str) {
    write_fake_executable(
        path,
        &format!(
            r#"#!/bin/sh
case "$1" in
  --version)
    echo "{version} (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
  *)
    exit 2
    ;;
esac
"#
        ),
    );
}

#[cfg(unix)]
fn write_fake_codex(path: &Path) {
    write_fake_executable(
        path,
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "codex-cli 0.99.0"
    ;;
  --help)
    echo "Usage: codex [options] <prompt>"
    echo "Commands: exec resume mcp"
    echo "Options: --config --model --sandbox --add-dir --search"
    ;;
  exec)
    echo "Run Codex non-interactively"
    echo "Options: --config --model --sandbox --add-dir --json"
    ;;
  *)
    exit 2
    ;;
esac
"#,
    );
}

fn claude_probe(
    probes: &mut HashMap<AgentHarnessKind, HarnessRuntimeProbe>,
) -> HarnessRuntimeProbe {
    probes
        .remove(&AgentHarnessKind::Claude)
        .expect("Claude probe should be present")
}

fn model_aliases(probe: &HarnessRuntimeProbe) -> Vec<String> {
    probe.supported_model_aliases.clone().unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn refresh_supported_harnesses_reprobes_claude_aliases_after_cli_update() {
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    let claude_cli = bin_dir.join("claude");
    let codex_cli = bin_dir.join("codex");
    write_fake_claude(&claude_cli, "2.1.110");
    write_fake_codex(&codex_cli);

    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _home = EnvGuard::set_os("HOME", temp.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
    let _nvm = EnvGuard::unset("NVM_BIN");
    let _volta = EnvGuard::unset("VOLTA_HOME");

    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Claude);
    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Codex);

    let initial_probe = refresh_harness_runtime_probe(AgentHarnessKind::Claude);
    assert_eq!(initial_probe.cli_version.as_deref(), Some("2.1.110"));
    assert!(!model_aliases(&initial_probe).contains(&"claude-opus-4-7".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-opus-4-8".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-opus-5".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"fable".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-sonnet-4-6".to_string()));
    assert!(!model_aliases(&initial_probe).contains(&"claude-sonnet-5".to_string()));

    write_fake_claude(&claude_cli, "2.1.219");
    let cached_probe = claude_probe(&mut probe_supported_harnesses());
    assert_eq!(cached_probe.cli_version.as_deref(), Some("2.1.110"));
    assert!(!model_aliases(&cached_probe).contains(&"claude-opus-4-7".to_string()));
    assert!(!model_aliases(&cached_probe).contains(&"claude-opus-4-8".to_string()));
    assert!(!model_aliases(&cached_probe).contains(&"claude-opus-5".to_string()));
    assert!(!model_aliases(&cached_probe).contains(&"fable".to_string()));

    let refreshed_probe = claude_probe(&mut refresh_supported_harnesses());
    assert_eq!(refreshed_probe.cli_version.as_deref(), Some("2.1.219"));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-opus-4-7".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-opus-4-8".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-opus-5".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"fable".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-sonnet-4-6".to_string()));
    assert!(model_aliases(&refreshed_probe).contains(&"claude-sonnet-5".to_string()));

    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Claude);
    clear_harness_runtime_caches_for_tests(AgentHarnessKind::Codex);
}
