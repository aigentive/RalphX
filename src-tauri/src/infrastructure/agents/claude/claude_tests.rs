use super::*;
use crate::utils::path_safety::{checked_exists, checked_read_to_string};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// Regression: the `--permission-prompt-tool` flag for an external-transport agent
/// (mixed external + internal MCP) must name the internal sidecar server, matching
/// the `permission_request` tool injected into `--allowed-tools`. Otherwise the
/// Claude CLI aborts before any MCP tool (e.g. ideation start) can run.
#[test]
fn test_resolve_claude_permission_cli_options_external_agent_uses_internal_server() {
    let options = resolve_claude_permission_cli_options(Some("ralphx-chat-project"), None);
    assert_eq!(
        options.permission_prompt_tool,
        "mcp__ralphx_internal__permission_request"
    );

    // The flag value must be present in the agent's pre-approved tool surface.
    let preapproved = get_preapproved_tools("ralphx-chat-project").unwrap();
    let tool_list: std::collections::HashSet<_> = preapproved.split(',').collect();
    assert!(tool_list.contains(options.permission_prompt_tool.as_str()));
}

/// Non-external agents keep the primary-server permission-prompt tool unchanged.
#[test]
fn test_resolve_claude_permission_cli_options_worker_uses_primary_server() {
    let options = resolve_claude_permission_cli_options(Some("ralphx-execution-worker"), None);
    assert_eq!(
        options.permission_prompt_tool,
        "mcp__ralphx__permission_request"
    );
}

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

fn read_test_file(path: impl AsRef<Path>) -> String {
    checked_read_to_string(path.as_ref(), "Claude plugin test fixture")
        .expect("read Claude plugin test fixture")
}

fn test_path_exists(path: impl AsRef<Path>) -> bool {
    checked_exists(path.as_ref(), "Claude plugin test fixture")
        .expect("inspect Claude plugin test fixture")
}

#[cfg(unix)]
fn write_fake_claude_cli(path: &Path) {
    std::fs::write(
        path,
        r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.219 (Claude Code)" ;;
  --help) echo "Options:" ;;
  *) exit 2 ;;
esac
"#,
    )
    .expect("write fake Claude CLI");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = std::fs::metadata(path)
        .expect("fake Claude CLI metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("mark fake Claude CLI executable");
}

fn path_index(entries: &[PathBuf], path: impl AsRef<Path>) -> usize {
    entries
        .iter()
        .position(|entry| entry == path.as_ref())
        .unwrap_or_else(|| panic!("PATH entry missing: {}", path.as_ref().display()))
}

/// build_spawnable_command calls ensure_claude_spawn_allowed() which returns
/// Err in tests — exercise the function up to that guard.
#[test]
fn test_build_spawnable_command_blocked_in_tests() {
    let result = build_spawnable_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        "test prompt",
        None,
        None,
        Path::new("/tmp"),
        None,
        None,
    );
    // In test env, ensure_claude_spawn_allowed() returns Err
    assert!(result.is_err(), "should be blocked in test environment");
    assert!(
        result.unwrap_err().contains("disabled"),
        "error should mention spawn disabled"
    );
}

/// build_spawnable_interactive_command is also blocked in tests by the same guard.
#[test]
fn test_build_spawnable_interactive_command_blocked_in_tests() {
    let result = build_spawnable_interactive_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        "my interactive prompt",
        None,
        None,
        Path::new("/tmp"),
        false,
        None,
        None,
    );
    assert!(result.is_err(), "should be blocked in test environment");
}

/// Verify SpawnableCommand::spawn_interactive is a method that exists and the type
/// compiles correctly. The actual spawn is gated behind ensure_claude_spawn_allowed.
#[test]
fn test_spawnable_command_debug_impl() {
    fn assert_debug<T: std::fmt::Debug>() {}
    assert_debug::<SpawnableCommand>();
}

#[test]
fn spawnable_command_debug_redacts_env_values() {
    let mut command = Command::new("/fake/claude");
    command.env("ANTHROPIC_AUTH_TOKEN", "secret-token");
    let spawnable = SpawnableCommand::new(command, None);

    let debug = format!("{spawnable:?}");

    assert!(debug.contains("ANTHROPIC_AUTH_TOKEN"));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("secret-token"));
}

#[test]
fn common_spawn_env_sets_agent_tool_path() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let seeded_path = dirs::home_dir()
        .map(|home| {
            std::env::join_paths([
                home.join(".cargo").join("bin"),
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ])
            .expect("seed test PATH")
        })
        .unwrap_or_else(|| OsString::from("/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"));
    let _path = EnvGuard::set_os("PATH", seeded_path);
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");

    let mut command = Command::new("/fake/claude");
    apply_common_spawn_env(&mut command);

    let path = command
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == "PATH").then(|| value.map(|path| path.to_string_lossy().into_owned()))?
        })
        .expect("PATH should be explicitly set for agent subprocesses");

    assert!(path.contains("/opt/homebrew/bin"));
    assert!(path.contains("/usr/local/bin"));
    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();
        assert!(
            path_index(&entries, &cargo_bin) < path_index(&entries, "/opt/homebrew/bin"),
            "user cargo shim should stay before Homebrew in Claude spawn PATH: {path}"
        );
    }

    let screenshot_dir = command
        .as_std()
        .get_envs()
        .find_map(|(key, value)| {
            (key == "RALPHX_AGENT_SCREENSHOT_DIR")
                .then(|| value.map(|path| path.to_string_lossy().into_owned()))?
        })
        .expect("RALPHX_AGENT_SCREENSHOT_DIR should be explicitly set");
    assert!(screenshot_dir.contains("screenshots"));
}

#[test]
fn test_apply_common_spawn_env_preserves_user_shims_while_ensuring_node_bin() {
    let _lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let _path = EnvGuard::set_os("PATH", "/usr/bin:/bin");
    let _disable_login_shell =
        EnvGuard::set_os(crate::infrastructure::login_shell_env::DISABLE_ENV_VAR, "1");
    let _node_override = EnvGuard::set_os("RALPHX_NODE_PATH", "/tmp/fake-node-bin/node");
    let expected_node_bin = PathBuf::from("/tmp/fake-node-bin");

    let mut cmd = Command::new("/usr/bin/env");
    apply_common_spawn_env(&mut cmd);

    let envs = cmd
        .as_std()
        .get_envs()
        .filter_map(|(key, value)| value.map(|val| (key.to_os_string(), val.to_os_string())))
        .collect::<Vec<_>>();
    let path_value = envs
        .iter()
        .find(|(key, _)| key == OsStr::new("PATH"))
        .map(|(_, value)| value.clone())
        .expect("PATH env");
    let path_entries = std::env::split_paths(&path_value).collect::<Vec<_>>();

    if let Some(home) = dirs::home_dir() {
        let cargo_bin = home.join(".cargo").join("bin");
        assert!(
            path_index(&path_entries, &cargo_bin) < path_index(&path_entries, &expected_node_bin),
            "user cargo shim should stay before inserted Node bin: {path_value:?}"
        );
    }
    assert!(path_index(&path_entries, &expected_node_bin) < path_index(&path_entries, "/usr/bin"));
}

/// build_base_cli_command with is_external_mcp=true is also blocked in tests by the
/// same spawn guard. The env var propagation logic (RALPHX_IS_EXTERNAL_TRIGGER=1)
/// executes after the guard; this test confirms the function accepts the flag and
/// returns the expected blocked error in the test environment.
#[test]
fn test_build_base_cli_command_external_mcp_blocked_in_tests() {
    let result = build_base_cli_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        None,
        true, // is_external_mcp=true
        None,
        None,
    );
    assert!(result.is_err(), "should be blocked in test environment");
    assert!(
        result.unwrap_err().contains("disabled"),
        "error should mention spawn disabled"
    );
}

/// build_base_cli_command with is_external_mcp=false is also blocked in tests.
#[test]
fn test_build_base_cli_command_internal_mcp_blocked_in_tests() {
    let result = build_base_cli_command(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        None,
        false, // is_external_mcp=false
        None,
        None,
    );
    assert!(result.is_err(), "should be blocked in test environment");
}

#[test]
fn test_build_base_cli_command_defaults_to_most_permissive_claude_permissions() {
    let _lock = lock_runtime_plugin_dirs_for_tests();
    let command = build_base_cli_command_inner(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        None,
        false,
        None,
        None,
        false,
    )
    .expect("build base command with spawn guard disabled");
    let args = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let permission_mode_idx = args
        .iter()
        .position(|arg| arg == "--permission-mode")
        .expect("--permission-mode flag");

    assert_eq!(args[permission_mode_idx + 1], "bypassPermissions");
    assert!(
        args.contains(&"--dangerously-skip-permissions".to_string()),
        "Claude base command must bypass permission prompts by default"
    );
}

#[cfg(unix)]
#[test]
fn test_build_base_cli_command_preserves_supported_model_values_byte_for_byte() {
    let _lock = lock_runtime_plugin_dirs_for_tests();
    let temp_dir = tempfile::tempdir().expect("temp dir");
    let cli_path = temp_dir.path().join("claude");
    write_fake_claude_cli(&cli_path);
    clear_claude_cli_capability_cache();

    for model in [
        "sonnet",
        "opus",
        "haiku",
        "fable",
        "claude-opus-4-7",
        "claude-opus-4-8",
        "claude-opus-5",
    ] {
        let command = build_base_cli_command_inner(
            &cli_path,
            Path::new("/fake/plugin"),
            None,
            false,
            None,
            Some(model),
            false,
        )
        .expect("supported model should build base command");
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let model_index = args
            .iter()
            .position(|arg| arg == "--model")
            .expect("--model flag");

        assert_eq!(args.get(model_index + 1).map(String::as_str), Some(model));
    }

    clear_claude_cli_capability_cache();
}

#[test]
fn test_build_base_cli_command_uses_provider_permission_override() {
    let _lock = lock_runtime_plugin_dirs_for_tests();
    let previous = set_claude_permission_runtime_override(Some(ClaudePermissionRuntimeOverride {
        permission_mode: Some("dontAsk".to_string()),
        dangerously_skip_permissions: false,
        allow_dangerously_skip_permissions: true,
    }));

    let command = build_base_cli_command_inner(
        Path::new("/fake/claude"),
        Path::new("/fake/plugin"),
        None,
        false,
        None,
        None,
        false,
    )
    .expect("build base command with spawn guard disabled");
    let args = command
        .as_std()
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let permission_mode_idx = args
        .iter()
        .position(|arg| arg == "--permission-mode")
        .expect("--permission-mode flag");

    assert_eq!(args[permission_mode_idx + 1], "dontAsk");
    assert!(args.contains(&"--allow-dangerously-skip-permissions".to_string()));
    assert!(!args.contains(&"--dangerously-skip-permissions".to_string()));

    set_claude_permission_runtime_override(previous);
}

#[test]
fn test_resolve_plugin_dir_uses_configured_runtime_root_not_target_project() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let runtime_root = temp_dir.path().join("runtime");
    let runtime_plugin_dir = runtime_root.join(PRIMARY_PLUGIN_DIR_REL);
    let generated_plugin_dir = temp_dir.path().join("generated/claude-plugin");
    let target_project_dir = temp_dir.path().join("target-project");
    std::fs::create_dir_all(runtime_plugin_dir.join("agents")).unwrap();
    std::fs::create_dir_all(target_project_dir.join(PRIMARY_PLUGIN_DIR_REL)).unwrap();
    let _guard = override_runtime_plugin_dirs_for_tests(
        runtime_plugin_dir.clone(),
        generated_plugin_dir.clone(),
    );

    assert_eq!(
        resolve_base_plugin_dir(&target_project_dir),
        runtime_plugin_dir
    );
    assert_eq!(
        resolve_plugin_dir(&target_project_dir),
        generated_plugin_dir
    );
}

#[test]
fn test_resolve_base_plugin_dir_falls_back_to_source_runtime_root() {
    let resolved = resolve_base_plugin_dir(Path::new("/tmp/target-project"));
    assert!(
        resolved.ends_with(PRIMARY_PLUGIN_DIR_REL) || resolved.ends_with(LEGACY_PLUGIN_DIR_REL),
        "unexpected runtime plugin dir: {}",
        resolved.display()
    );
}

#[test]
fn test_materialize_generated_plugin_dir_generates_canonical_agents_and_preserves_runtime_assets() {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let repo_root = temp_dir.path().join("repo");
    let plugin_dir = repo_root.join(PRIMARY_PLUGIN_DIR_REL);
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build")).unwrap();
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake",
    )
    .unwrap();
    std::fs::create_dir_all(repo_root.join("agents/ralphx-utility-session-namer/shared")).unwrap();
    std::fs::write(
        repo_root.join("agents/ralphx-utility-session-namer/agent.yaml"),
        "name: ralphx-utility-session-namer\nrole: session_namer\n",
    )
    .unwrap();
    std::fs::write(
        repo_root.join("agents/ralphx-utility-session-namer/shared/prompt.md"),
        "Canonical Session Namer Prompt",
    )
    .unwrap();

    let generated_dir =
        materialize_generated_plugin_dir(&plugin_dir).expect("generated plugin dir");
    let generated_session_namer =
        read_test_file(generated_dir.join("agents/ralphx-utility-session-namer.md"));
    assert!(
        generated_session_namer.contains("Canonical Session Namer Prompt"),
        "generated session namer should use canonical prompt body"
    );
    assert!(
        generated_session_namer.contains("name: ralphx-utility-session-namer"),
        "generated session namer should render Claude frontmatter"
    );
    assert!(
        !test_path_exists(generated_dir.join("agents/worker.md")),
        "generated plugin should not carry non-canonical legacy plugin prompt files"
    );
    assert!(
        test_path_exists(generated_dir.join("ralphx-mcp-server/build/index.js")),
        "generated plugin dir should keep MCP runtime assets available"
    );
    assert!(
        !test_path_exists(generated_dir.join(".mcp.json")),
        "generated plugin must not materialize an ambient ralphx MCP registration; Claude spawns receive ralphx only through dynamic --mcp-config"
    );
}
