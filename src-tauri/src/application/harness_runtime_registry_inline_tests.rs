use super::*;
use std::ffi::OsStr;
use tempfile::TempDir;

fn plugin_override_lock() -> &'static std::sync::Mutex<()> {
    &HARNESS_RUNTIME_TEST_MUTEX
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

fn make_runtime_plugin_layout() -> (TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    let plugin_dir = root.join("plugins/app");
    let generated_dir = root.join("generated/claude-plugin");

    std::fs::create_dir_all(plugin_dir.join("agents")).expect("create agents dir");
    std::fs::write(
        plugin_dir.join("agents/session-namer.md"),
        "# Session Namer\n",
    )
    .expect("write session namer prompt");
    std::fs::create_dir_all(plugin_dir.join("ralphx-mcp-server/build"))
        .expect("create mcp build dir");
    std::fs::create_dir_all(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk"),
    )
    .expect("create mcp sdk marker dir");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/build/index.js"),
        "// fake mcp runtime\n",
    )
    .expect("write mcp runtime entry");
    std::fs::write(
        plugin_dir.join("ralphx-mcp-server/node_modules/@modelcontextprotocol/sdk/package.json"),
        "{}\n",
    )
    .expect("write mcp runtime marker");

    (temp, plugin_dir, generated_dir)
}

fn test_codex_capabilities() -> CodexCliCapabilities {
    CodexCliCapabilities {
        version: Some("0.124.0".to_string()),
        supports_exec_subcommand: true,
        supports_json_output: true,
        supports_model_flag: true,
        supports_config_override: true,
        supports_sandbox_flag: true,
        supports_add_dir: true,
        supports_search_flag: true,
        supports_resume_subcommand: true,
        supports_mcp_subcommand: true,
        supports_fast_mode_feature: true,
        fast_mode_supported_models: vec!["gpt-5.5".to_string()],
        supported_model_aliases: vec!["gpt-5.5".to_string()],
        supported_efforts: vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
        ],
        model_supported_efforts: std::collections::BTreeMap::new(),
        ultra_supported_models: Vec::new(),
    }
}

fn test_resolved_codex_cli(path: &str) -> ResolvedCodexCli {
    ResolvedCodexCli {
        path: PathBuf::from(path),
        capabilities: test_codex_capabilities(),
    }
}

#[test]
fn default_chat_service_cli_name_matches_standard_harnesses() {
    assert_eq!(
        default_chat_service_cli_name(AgentHarnessKind::Claude),
        "claude"
    );
    assert_eq!(
        default_chat_service_cli_name(AgentHarnessKind::Codex),
        "codex"
    );
}

#[test]
fn resolve_default_chat_service_bootstrap_uses_default_harness() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
        cache.lock().unwrap().clear();
    }
    if let Some(cache) = CHAT_HARNESS_CLI_CACHE.get() {
        cache.lock().unwrap().clear();
    }

    assert_eq!(
        resolve_default_chat_service_bootstrap(),
        resolve_chat_service_bootstrap(DEFAULT_AGENT_HARNESS)
    );
}

#[test]
fn codex_chat_harness_cli_maps_compatible_default_candidate() {
    let resolved = codex_chat_harness_cli_from_resolve_result(Ok(test_resolved_codex_cli(
        "/opt/homebrew/bin/codex",
    )))
    .unwrap();

    match resolved {
        ResolvedChatHarnessCli::Codex {
            cli_path,
            capabilities,
        } => {
            assert_eq!(cli_path, PathBuf::from("/opt/homebrew/bin/codex"));
            assert!(capabilities.has_core_exec_support());
        }
        ResolvedChatHarnessCli::Claude { .. } => panic!("expected Codex CLI resolution"),
    }
}

#[test]
fn chat_harness_cli_resolution_uses_app_session_caches() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    if let Some(cache) = CODEX_CLI_CAPABILITY_CACHE.get() {
        cache.lock().unwrap().clear();
    }
    let temp = tempfile::tempdir().expect("tempdir");
    let claude_cli = temp.path().join("claude");
    let codex_cli = temp.path().join("codex");
    std::fs::write(&claude_cli, "#!/bin/sh\n").expect("write fake claude");
    std::fs::write(&codex_cli, "#!/bin/sh\n").expect("write fake codex");

    let claude = resolve_claude_chat_harness_cli(&claude_cli)
        .expect("fake existing Claude CLI path should resolve");
    match claude {
        ResolvedChatHarnessCli::Claude { cli_path } => assert_eq!(cli_path, claude_cli),
        ResolvedChatHarnessCli::Codex { .. } => panic!("expected Claude CLI"),
    }

    CODEX_CLI_CAPABILITY_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(codex_cli.clone(), Ok(test_codex_capabilities()));
    let codex = resolve_codex_chat_harness_cli(&codex_cli)
        .expect("cached fake Codex CLI path should resolve");
    match codex {
        ResolvedChatHarnessCli::Codex {
            cli_path,
            capabilities,
        } => {
            assert_eq!(cli_path, codex_cli);
            assert!(capabilities.has_core_exec_support());
        }
        ResolvedChatHarnessCli::Claude { .. } => panic!("expected Codex CLI"),
    }

    let missing = resolve_codex_chat_harness_cli(&temp.path().join("missing-codex"))
        .expect_err("missing explicit Codex path should fail");
    assert!(missing.contains("Codex CLI not found"));

    CODEX_CLI_CAPABILITY_CACHE
        .get()
        .expect("capability cache should exist")
        .lock()
        .unwrap()
        .clear();
}

#[test]
fn codex_resolution_cache_is_reused_for_probe_and_capabilities() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    let resolved = test_resolved_codex_cli("/tmp/cached-codex");
    *RESOLVED_CODEX_CLI_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(Ok(resolved.clone()));

    let cached = resolve_codex_cli_cached().expect("cached Codex resolution should return");
    assert_eq!(cached.path, resolved.path);
    let capabilities =
        probe_codex_cli_cached(&resolved.path).expect("resolved capabilities should be reused");
    assert!(capabilities.has_core_exec_support());

    let (probe, returned_capabilities) = probe_codex_harness_with_capabilities();
    let expected_path = resolved.path.to_string_lossy().to_string();
    assert!(probe.available);
    assert_eq!(probe.binary_path.as_deref(), Some(expected_path.as_str()));
    assert!(returned_capabilities
        .expect("capabilities should be returned")
        .has_core_exec_support());

    *RESOLVED_CODEX_CLI_CACHE
        .get()
        .expect("Codex resolution cache should exist")
        .lock()
        .unwrap() = None;
}

#[test]
fn harness_probe_and_chat_cli_resolution_cache_results() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
        cache.lock().unwrap().clear();
    }
    if let Some(cache) = CHAT_HARNESS_CLI_CACHE.get() {
        cache.lock().unwrap().clear();
    }
    HARNESS_RUNTIME_PROBE_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(
            AgentHarnessKind::Claude,
            HarnessRuntimeProbe {
                binary_path: Some("/tmp/cached-claude".to_string()),
                binary_found: true,
                probe_succeeded: true,
                available: true,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: None,
            },
        );
    assert_eq!(
        probe_harness(AgentHarnessKind::Claude)
            .binary_path
            .as_deref(),
        Some("/tmp/cached-claude")
    );

    let temp = tempfile::tempdir().expect("tempdir");
    let claude_cli = temp.path().join("claude");
    std::fs::write(&claude_cli, "#!/bin/sh\n").expect("write fake claude");
    let first = resolve_chat_harness_cli(AgentHarnessKind::Claude, &claude_cli)
        .expect("Claude chat CLI should resolve");
    let second = resolve_chat_harness_cli(AgentHarnessKind::Claude, &claude_cli)
        .expect("cached Claude chat CLI should resolve");

    match (first, second) {
        (
            ResolvedChatHarnessCli::Claude { cli_path: first },
            ResolvedChatHarnessCli::Claude { cli_path: second },
        ) => assert_eq!(first, second),
        _ => panic!("expected Claude CLI results"),
    }

    HARNESS_RUNTIME_PROBE_CACHE
        .get()
        .expect("probe cache should exist")
        .lock()
        .unwrap()
        .clear();
    CHAT_HARNESS_CLI_CACHE
        .get()
        .expect("chat CLI cache should exist")
        .lock()
        .unwrap()
        .clear();
}

#[test]
fn harness_probe_reuses_in_flight_probe_result() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    if let Some(cache) = HARNESS_RUNTIME_PROBE_CACHE.get() {
        cache.lock().unwrap().clear();
    }
    if let Some(in_flight) = HARNESS_RUNTIME_PROBE_IN_FLIGHT.get() {
        in_flight.lock().unwrap().clear();
    }

    let expected = HarnessRuntimeProbe {
        binary_path: Some("/tmp/in-flight-claude".to_string()),
        binary_found: true,
        probe_succeeded: true,
        available: true,
        missing_core_exec_features: Vec::new(),
        cli_version: None,
        supported_model_aliases: None,
        supported_efforts: None,
        ultra_supported_models: Vec::new(),
        supports_fast_mode: false,
        fast_mode_supported_models: Vec::new(),
        error: None,
    };
    let probe_in_flight = Arc::new(HarnessRuntimeProbeInFlight::new());
    {
        let mut result = probe_in_flight.result.lock().unwrap();
        *result = Some(expected.clone());
    }
    HARNESS_RUNTIME_PROBE_IN_FLIGHT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(AgentHarnessKind::Claude, probe_in_flight);

    assert_eq!(probe_harness(AgentHarnessKind::Claude), expected);

    HARNESS_RUNTIME_PROBE_IN_FLIGHT
        .get()
        .expect("in-flight probe map should exist")
        .lock()
        .unwrap()
        .clear();
}

#[cfg(unix)]
#[test]
fn claude_harness_probe_reports_cli_supported_efforts() {
    let _plugin_lock = plugin_override_lock().lock().expect("lock harness caches");
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_executable(
        &bin_dir.join("claude"),
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.142 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
  *)
    exit 2
    ;;
esac
"#,
    );
    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _home = EnvGuard::set_os("HOME", temp.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
    let _nvm = EnvGuard::unset("NVM_BIN");
    let _volta = EnvGuard::unset("VOLTA_HOME");

    let probe = probe_claude_harness();

    assert!(probe.available);
    assert!(probe.probe_succeeded);
    assert_eq!(probe.cli_version.as_deref(), Some("2.1.142"));
    assert_eq!(
        probe.supported_model_aliases,
        Some(vec![
            "sonnet".to_string(),
            "opus".to_string(),
            "haiku".to_string(),
            "claude-opus-4-7".to_string(),
        ])
    );
    assert_eq!(
        probe.supported_efforts,
        Some(vec![
            "low".to_string(),
            "medium".to_string(),
            "high".to_string(),
            "xhigh".to_string(),
            "max".to_string(),
        ])
    );

    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
}

#[cfg(unix)]
#[test]
fn claude_harness_probe_keeps_binary_available_when_capability_probe_fails() {
    let _plugin_lock = plugin_override_lock().lock().expect("lock harness caches");
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
    let temp = tempfile::tempdir().expect("tempdir");
    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    write_fake_executable(
        &bin_dir.join("claude"),
        r#"#!/bin/sh
echo "probe failed" >&2
exit 42
"#,
    );
    let _path = EnvGuard::set_os("PATH", &bin_dir);
    let _home = EnvGuard::set_os("HOME", temp.path());
    let _zdotdir = EnvGuard::set_os("ZDOTDIR", temp.path());
    let _nvm = EnvGuard::unset("NVM_BIN");
    let _volta = EnvGuard::unset("VOLTA_HOME");

    let probe = probe_claude_harness();

    assert!(probe.binary_found);
    assert!(probe.available);
    assert!(!probe.probe_succeeded);
    assert_eq!(probe.supported_efforts, None);
    assert!(probe
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("probe failed"));

    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
}

#[cfg(unix)]
#[test]
fn clearing_claude_runtime_caches_removes_cached_cli_capabilities() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    let _env_lock = crate::infrastructure::tool_paths::TEST_ENV_MUTEX
        .lock()
        .expect("env mutex");
    let temp = tempfile::tempdir().expect("tempdir");
    let cli_path = temp.path().join("claude");
    write_fake_executable(
        &cli_path,
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.142 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, xhigh, max)"
    ;;
esac
"#,
    );

    assert_eq!(
        crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
            &cli_path, "xhigh",
        ),
        "xhigh"
    );

    write_fake_executable(
        &cli_path,
        r#"#!/bin/sh
case "$1" in
  --version)
    echo "2.1.110 (Claude Code)"
    ;;
  --help)
    echo "Options:"
    echo "  --effort <level>  Effort level for the current session (low, medium, high, max)"
    ;;
esac
"#,
    );
    assert_eq!(
        crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
            &cli_path, "xhigh",
        ),
        "xhigh"
    );

    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);

    assert_eq!(
        crate::infrastructure::agents::claude::normalize_claude_effort_for_cli_path(
            &cli_path, "xhigh",
        ),
        "high"
    );

    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Claude);
}

#[test]
fn clearing_codex_runtime_caches_removes_probe_cli_and_capability_entries() {
    let _lock = plugin_override_lock().lock().expect("lock harness caches");
    let codex_path = PathBuf::from("/tmp/codex-cache-test");
    HARNESS_RUNTIME_PROBE_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(
            AgentHarnessKind::Codex,
            HarnessRuntimeProbe {
                binary_path: Some(codex_path.display().to_string()),
                binary_found: true,
                probe_succeeded: true,
                available: true,
                missing_core_exec_features: Vec::new(),
                cli_version: None,
                supported_model_aliases: None,
                supported_efforts: None,
                ultra_supported_models: Vec::new(),
                supports_fast_mode: false,
                fast_mode_supported_models: Vec::new(),
                error: None,
            },
        );
    CHAT_HARNESS_CLI_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(
            (AgentHarnessKind::Codex, codex_path.clone()),
            Ok(ResolvedChatHarnessCli::Codex {
                cli_path: codex_path.clone(),
                capabilities: test_codex_capabilities(),
            }),
        );
    *RESOLVED_CODEX_CLI_CACHE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap() = Some(Ok(ResolvedCodexCli {
        path: codex_path.clone(),
        capabilities: test_codex_capabilities(),
    }));
    CODEX_CLI_CAPABILITY_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap()
        .insert(codex_path, Ok(test_codex_capabilities()));

    clear_harness_runtime_caches_for_harness(AgentHarnessKind::Codex);

    assert!(!HARNESS_RUNTIME_PROBE_CACHE
        .get()
        .expect("probe cache should exist")
        .lock()
        .unwrap()
        .contains_key(&AgentHarnessKind::Codex));
    assert!(CHAT_HARNESS_CLI_CACHE
        .get()
        .expect("chat CLI cache should exist")
        .lock()
        .unwrap()
        .is_empty());
    assert!(RESOLVED_CODEX_CLI_CACHE
        .get()
        .expect("Codex resolution cache should exist")
        .lock()
        .unwrap()
        .is_none());
    assert!(CODEX_CLI_CAPABILITY_CACHE
        .get()
        .expect("Codex capability cache should exist")
        .lock()
        .unwrap()
        .is_empty());
}

#[test]
fn codex_chat_service_cli_path_uses_compatible_candidate() {
    let cli_path = codex_chat_service_cli_path_from_resolve_result(Ok(test_resolved_codex_cli(
        "/opt/homebrew/bin/codex",
    )));

    assert_eq!(cli_path, PathBuf::from("/opt/homebrew/bin/codex"));
}

#[test]
fn codex_chat_service_cli_path_falls_back_to_default_name_when_resolution_fails() {
    let cli_path =
        codex_chat_service_cli_path_from_resolve_result(Err("Codex CLI not found".to_string()));

    assert_eq!(cli_path, PathBuf::from("codex"));
}

#[test]
fn default_repo_root_working_directory_uses_parent_for_src_tauri() {
    let cwd = PathBuf::from("/tmp/example/src-tauri");
    assert_eq!(
        default_repo_root_working_directory_from(cwd),
        PathBuf::from("/tmp/example")
    );
}

#[test]
fn default_repo_root_working_directory_keeps_non_src_tauri_paths() {
    let cwd = PathBuf::from("/tmp/example");
    assert_eq!(default_repo_root_working_directory_from(cwd.clone()), cwd);
}

#[test]
fn external_mcp_entry_for_plugin_dir_appends_expected_relative_path() {
    let plugin_dir = PathBuf::from("/tmp/plugins/app");
    assert_eq!(
        external_mcp_entry_for_plugin_dir(&plugin_dir),
        plugin_dir.join("ralphx-external-mcp/build/index.js")
    );
}

#[test]
fn resolve_default_harness_agent_bootstrap_sets_expected_defaults() {
    let _lock = plugin_override_lock().lock().expect("lock plugin override");
    let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir,
        );
    let working_directory = PathBuf::from("/tmp/example");
    let agent_name = crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER;
    let bootstrap = resolve_harness_agent_bootstrap(
        DEFAULT_AGENT_HARNESS,
        agent_name,
        working_directory.clone(),
    );

    assert_eq!(bootstrap.agent_name, agent_name);
    assert_eq!(bootstrap.agent_role, "ralphx-utility-session-namer");
    assert_eq!(bootstrap.working_directory, working_directory);
    assert_eq!(
        bootstrap.env.get("RALPHX_AGENT_TYPE"),
        Some(&"ralphx-utility-session-namer".to_string())
    );
    assert_eq!(
        bootstrap.plugin_dir,
        resolve_default_harness_plugin_dir(&bootstrap.working_directory)
    );
}

#[test]
fn resolve_harness_agent_bootstrap_uses_harness_plugin_dir_resolution() {
    let _lock = plugin_override_lock().lock().expect("lock plugin override");
    let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir,
        );
    let working_directory = PathBuf::from("/tmp/example");
    let agent_name = crate::infrastructure::agents::claude::agent_names::AGENT_SESSION_NAMER;
    let bootstrap = resolve_harness_agent_bootstrap(
        AgentHarnessKind::Codex,
        agent_name,
        working_directory.clone(),
    );

    assert_eq!(bootstrap.agent_name, agent_name);
    assert_eq!(bootstrap.agent_role, "ralphx-utility-session-namer");
    assert_eq!(bootstrap.working_directory, working_directory);
    assert_eq!(
        bootstrap.plugin_dir,
        resolve_harness_plugin_dir(AgentHarnessKind::Codex, &bootstrap.working_directory)
    );
}

#[test]
fn resolve_harness_plugin_dir_uses_generated_plugin_dir_for_codex() {
    let _lock = plugin_override_lock().lock().expect("lock plugin override");
    let (_temp, plugin_dir, generated_dir) = make_runtime_plugin_layout();
    let _runtime_guard =
        crate::infrastructure::agents::claude::override_runtime_plugin_dirs_for_tests(
            plugin_dir,
            generated_dir.clone(),
        );
    let working_directory = PathBuf::from("/tmp/example");

    assert_eq!(
        resolve_harness_plugin_dir(AgentHarnessKind::Codex, &working_directory),
        generated_dir
    );
    assert_eq!(
        resolve_default_harness_plugin_dir(&working_directory),
        generated_dir
    );
}
