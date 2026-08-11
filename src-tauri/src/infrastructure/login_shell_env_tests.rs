use crate::infrastructure::login_shell_env;
use std::collections::HashMap;
use std::ffi::OsStr;

#[test]
fn parse_env_dump_basic_key_value_pairs() {
    let dump = "ANTHROPIC_API_KEY=sk-ant-foo\nOPENAI_API_KEY=sk-openai-bar\nHOME=/Users/test\n";
    let map = login_shell_env::parse_env_dump(dump);

    assert_eq!(
        map.get("ANTHROPIC_API_KEY").map(String::as_str),
        Some("sk-ant-foo")
    );
    assert_eq!(
        map.get("OPENAI_API_KEY").map(String::as_str),
        Some("sk-openai-bar")
    );
    assert_eq!(map.get("HOME").map(String::as_str), Some("/Users/test"));
}

#[test]
fn parse_env_dump_preserves_equals_in_value() {
    // Real-world: tokens, base64, key=value config strings, and PATH-style values
    // all contain `=`. Only the first `=` is the separator.
    let dump = "TOKEN=abc=def=ghi\nAWS_SESSION_TOKEN=AAAAB3NzaC1==\n";
    let map = login_shell_env::parse_env_dump(dump);

    assert_eq!(map.get("TOKEN").map(String::as_str), Some("abc=def=ghi"));
    assert_eq!(
        map.get("AWS_SESSION_TOKEN").map(String::as_str),
        Some("AAAAB3NzaC1==")
    );
}

#[test]
fn parse_env_dump_skips_lines_without_equals() {
    let dump = "GOOD=value\njunk-line-no-equals\nANOTHER=ok\n";
    let map = login_shell_env::parse_env_dump(dump);

    assert_eq!(map.get("GOOD").map(String::as_str), Some("value"));
    assert_eq!(map.get("ANOTHER").map(String::as_str), Some("ok"));
    assert_eq!(map.len(), 2);
}

#[test]
fn parse_env_dump_skips_bash_function_exports() {
    // bash exports declared functions as `BASH_FUNC_name%%=()...`. These are
    // not useful to a non-bash child and pollute the env if forwarded.
    let dump = "REAL_VAR=keep\nBASH_FUNC_foo%%=() { echo hi; }\n";
    let map = login_shell_env::parse_env_dump(dump);

    assert_eq!(map.get("REAL_VAR").map(String::as_str), Some("keep"));
    assert!(map.keys().all(|k| !k.starts_with("BASH_FUNC_")));
}

#[test]
fn parse_env_dump_allows_empty_value() {
    // `FOO=` with empty value should parse as ("FOO", "").
    let dump = "FOO=\nBAR=value\n";
    let map = login_shell_env::parse_env_dump(dump);

    assert_eq!(map.get("FOO").map(String::as_str), Some(""));
    assert_eq!(map.get("BAR").map(String::as_str), Some("value"));
}

#[test]
fn should_forward_blocks_ralphx_managed_path() {
    // PATH is managed by agent_subprocess_env_path() / configure_spawn — we must
    // not let the user's shell PATH overwrite the augmented PATH RalphX builds.
    assert!(!login_shell_env::should_forward_for_test("PATH"));
}

#[test]
fn captured_path_from_map_returns_path_without_forwarding_it() {
    let mut shell_env = HashMap::new();
    shell_env.insert(
        "PATH".to_string(),
        "/Users/example/.cargo/bin:/opt/homebrew/bin".to_string(),
    );

    assert_eq!(
        login_shell_env::captured_path_from_map_for_test(&shell_env).as_deref(),
        Some(OsStr::new("/Users/example/.cargo/bin:/opt/homebrew/bin"))
    );
    assert!(!login_shell_env::should_forward_for_test("PATH"));
}

#[test]
fn should_forward_blocks_ralphx_internal_namespaces() {
    // Anything prefixed with RALPHX_ is RalphX's own runtime signaling and must
    // not be re-injected from the user shell.
    assert!(!login_shell_env::should_forward_for_test("RALPHX_TASK_ID"));
    assert!(!login_shell_env::should_forward_for_test(
        "RALPHX_PROJECT_ID"
    ));
}

#[test]
fn should_forward_blocks_claude_runtime_overrides() {
    // These are set by apply_common_spawn_env to specific RalphX values — the
    // user's shell may have different values (e.g. CLAUDE_CODE_ENABLE_TASKS=0).
    assert!(!login_shell_env::should_forward_for_test(
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"
    ));
    assert!(!login_shell_env::should_forward_for_test(
        "CLAUDE_CODE_ENABLE_TASKS"
    ));
    assert!(!login_shell_env::should_forward_for_test(
        "CLAUDE_PLUGIN_ROOT"
    ));
    assert!(!login_shell_env::should_forward_for_test("TAURI_API_URL"));
    assert!(!login_shell_env::should_forward_for_test("DEBUG"));
}

#[test]
fn should_forward_blocks_toolchain_override_vars() {
    assert!(!login_shell_env::should_forward_for_test("RUSTC"));
    assert!(!login_shell_env::should_forward_for_test(
        "RUSTUP_TOOLCHAIN"
    ));
}

#[test]
fn should_forward_blocks_shell_state_keys() {
    // These reflect the shell's own runtime state, not user config.
    for key in ["_", "SHLVL", "OLDPWD", "PWD"] {
        assert!(
            !login_shell_env::should_forward_for_test(key),
            "shell-state key {key:?} must not be forwarded"
        );
    }
}

#[test]
fn should_forward_allows_auth_env_vars() {
    // The whole point of this module: provider auth env vars must reach the child.
    for key in [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_API_KEY",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "CODEX_HOME",
        "HOME",
    ] {
        assert!(
            login_shell_env::should_forward_for_test(key),
            "auth-related key {key:?} must be forwarded"
        );
    }
}

#[test]
fn should_forward_blocks_github_cli_token_vars_but_not_other_provider_auth() {
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        assert!(
            !login_shell_env::should_forward_for_test(key),
            "GitHub CLI token key {key:?} must not be forwarded"
        );
    }
    assert!(login_shell_env::should_forward_for_test("OPENAI_API_KEY"));
    assert!(login_shell_env::should_forward_for_test(
        "ANTHROPIC_API_KEY"
    ));
}

#[test]
fn should_forward_allows_github_cli_tokens_after_explicit_opt_out() {
    for key in [
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GH_ENTERPRISE_TOKEN",
        "GITHUB_ENTERPRISE_TOKEN",
    ] {
        assert!(
            login_shell_env::should_forward_with_github_token_removal_for_test(key, false),
            "GitHub CLI token key {key:?} must be forwarded after opt-out"
        );
    }
}

#[test]
fn apply_to_std_forwards_auth_vars_and_skips_managed_keys() {
    // Use the test override so we don't shell out. The override is process-wide
    // (OnceLock); other tests in this module avoid `captured()`, so installing
    // the override here is safe.
    let mut shell_env = HashMap::new();
    shell_env.insert(
        "ANTHROPIC_API_KEY".to_string(),
        "sk-ant-from-shell".to_string(),
    );
    shell_env.insert(
        "OPENAI_API_KEY".to_string(),
        "sk-openai-from-shell".to_string(),
    );
    shell_env.insert("MY_CUSTOM_VAR".to_string(), "user-defined".to_string());
    shell_env.insert("GITHUB_TOKEN".to_string(), "stale-secret".to_string());
    // These MUST NOT leak through — they would clobber RalphX-managed values
    // applied AFTER apply_to_std in claude/codex spawn helpers.
    shell_env.insert("PATH".to_string(), "/should/not/win".to_string());
    shell_env.insert("RUSTC".to_string(), "/opt/homebrew/bin/rustc".to_string());
    shell_env.insert("RUSTUP_TOOLCHAIN".to_string(), "1.85.1".to_string());
    shell_env.insert("CLAUDE_CODE_ENABLE_TASKS".to_string(), "0".to_string());
    shell_env.insert("RALPHX_TASK_ID".to_string(), "spoofed".to_string());
    shell_env.insert("SHLVL".to_string(), "2".to_string());
    login_shell_env::set_for_test(shell_env);

    let mut cmd = std::process::Command::new("/bin/true");
    login_shell_env::apply_to_std(&mut cmd);

    let envs: Vec<(String, Option<String>)> = cmd
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|os| os.to_string_lossy().into_owned()),
            )
        })
        .collect();

    let lookup = |key: &str| -> Option<String> {
        envs.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.clone())
    };

    assert_eq!(
        lookup("ANTHROPIC_API_KEY").as_deref(),
        Some("sk-ant-from-shell"),
        "user shell auth vars must reach spawned CLIs"
    );
    assert_eq!(
        lookup("OPENAI_API_KEY").as_deref(),
        Some("sk-openai-from-shell"),
    );
    assert_eq!(lookup("MY_CUSTOM_VAR").as_deref(), Some("user-defined"));

    assert!(
        envs.iter().all(|(k, _)| k.as_str() != "PATH"),
        "shell PATH must NOT be forwarded — RalphX's augmented PATH wins via the spawn helper applying after"
    );
    assert!(
        envs.iter()
            .all(|(k, _)| k.as_str() != "CLAUDE_CODE_ENABLE_TASKS"),
        "shell CLAUDE_CODE_ENABLE_TASKS must NOT be forwarded — RalphX sets it to 1 explicitly"
    );
    assert!(
        envs.iter().all(|(k, _)| k.as_str() != "RUSTC"),
        "shell RUSTC must not force provider descendants away from project rust-toolchain.toml"
    );
    assert!(
        envs.iter().all(|(k, _)| k.as_str() != "RUSTUP_TOOLCHAIN"),
        "shell RUSTUP_TOOLCHAIN must not force provider descendants away from project rust-toolchain.toml"
    );
    assert!(
        envs.iter().all(|(k, _)| k.as_str() != "RALPHX_TASK_ID"),
        "RALPHX_-prefixed vars must NOT be re-injected from the user shell"
    );
    assert!(
        envs.iter().all(|(k, _)| k.as_str() != "SHLVL"),
        "shell-state keys (SHLVL, _, PWD, OLDPWD) must NOT be forwarded"
    );
    assert!(
        envs.iter()
            .all(|(key, value)| key != "GITHUB_TOKEN" || value.is_none()),
        "GitHub CLI token vars must be explicitly removed from the child"
    );
}

#[tokio::test]
async fn apply_to_tokio_command_forwards_captured_env() {
    // apply_to (tokio variant) is a thin wrapper around apply_to_std. Smoke-test
    // it to make sure the wrapper actually reaches the underlying logic, and to
    // give the production claude/codex spawn helpers' call site coverage.
    let mut shell_env = HashMap::new();
    shell_env.insert("ANTHROPIC_API_KEY".to_string(), "via-tokio".to_string());
    login_shell_env::set_for_test(shell_env);

    let mut cmd = tokio::process::Command::new("/bin/true");
    login_shell_env::apply_to(&mut cmd);

    let std_cmd = cmd.as_std();
    let key_present = std_cmd
        .get_envs()
        .any(|(k, v)| k == OsStr::new("ANTHROPIC_API_KEY") && v.map(OsStr::to_os_string).is_some());
    assert!(
        key_present,
        "apply_to() must reach the same forwarding path as apply_to_std()"
    );
}

#[test]
fn disabled_by_env_reflects_env_var_state() {
    // Save and restore the env var to keep the test isolated.
    let prior = std::env::var_os(login_shell_env::DISABLE_ENV_VAR);

    std::env::remove_var(login_shell_env::DISABLE_ENV_VAR);
    assert!(
        !login_shell_env::disabled_by_env_for_test(),
        "unset env var means probe is enabled"
    );

    std::env::set_var(login_shell_env::DISABLE_ENV_VAR, "1");
    assert!(
        login_shell_env::disabled_by_env_for_test(),
        "any non-empty value of RALPHX_DISABLE_LOGIN_SHELL_ENV disables the probe"
    );

    std::env::set_var(login_shell_env::DISABLE_ENV_VAR, "");
    assert!(
        !login_shell_env::disabled_by_env_for_test(),
        "empty string must NOT disable the probe (matches the &OsStr non-empty check)"
    );

    match prior {
        Some(value) => std::env::set_var(login_shell_env::DISABLE_ENV_VAR, value),
        None => std::env::remove_var(login_shell_env::DISABLE_ENV_VAR),
    }
}

#[test]
fn resolve_login_shell_prefers_user_shell_env_var() {
    let prior = std::env::var_os("SHELL");

    std::env::set_var("SHELL", "/usr/bin/fish");
    assert_eq!(
        login_shell_env::resolve_login_shell_for_test().as_deref(),
        Some(OsStr::new("/usr/bin/fish")),
        "explicit $SHELL must win"
    );

    std::env::set_var("SHELL", "");
    let fallback = login_shell_env::resolve_login_shell_for_test();
    // Empty $SHELL falls through to the platform default.
    #[cfg(target_os = "macos")]
    assert_eq!(fallback.as_deref(), Some(OsStr::new("/bin/zsh")));
    #[cfg(all(unix, not(target_os = "macos")))]
    assert_eq!(fallback.as_deref(), Some(OsStr::new("/bin/bash")));
    #[cfg(not(unix))]
    assert!(fallback.is_none());

    std::env::remove_var("SHELL");
    let unset = login_shell_env::resolve_login_shell_for_test();
    #[cfg(target_os = "macos")]
    assert_eq!(unset.as_deref(), Some(OsStr::new("/bin/zsh")));
    #[cfg(all(unix, not(target_os = "macos")))]
    assert_eq!(unset.as_deref(), Some(OsStr::new("/bin/bash")));
    #[cfg(not(unix))]
    assert!(unset.is_none());

    match prior {
        Some(value) => std::env::set_var("SHELL", value),
        None => std::env::remove_var("SHELL"),
    }
}

#[test]
fn probe_shell_env_returns_empty_when_disabled() {
    // With the disable flag set, probe_shell_env must short-circuit and never
    // spawn a shell. The returned map must be empty regardless of the user's
    // real environment.
    let prior = std::env::var_os(login_shell_env::DISABLE_ENV_VAR);
    std::env::set_var(login_shell_env::DISABLE_ENV_VAR, "1");

    let map = login_shell_env::probe_shell_env_for_test();
    assert!(
        map.is_empty(),
        "disabled probe must return an empty map; got {} entries",
        map.len()
    );

    match prior {
        Some(value) => std::env::set_var(login_shell_env::DISABLE_ENV_VAR, value),
        None => std::env::remove_var(login_shell_env::DISABLE_ENV_VAR),
    }
}

#[test]
fn apply_to_std_does_not_clear_existing_env() {
    // The helper sets keys via `.env(key, value)` — it must NOT clear inherited env.
    // Inheritance is how things like `HOME` reach the child when the user's shell
    // probe fails or is disabled.
    let mut cmd = std::process::Command::new("/bin/true");
    cmd.env("PRE_EXISTING_VAR", "kept");
    login_shell_env::apply_to_std(&mut cmd);

    let pre_existing = cmd
        .get_envs()
        .find(|(k, _)| *k == OsStr::new("PRE_EXISTING_VAR"));
    assert!(
        pre_existing.is_some(),
        "apply_to_std must not call .env_clear() — caller-set env survives"
    );
}

#[test]
fn managed_keys_includes_path_and_claude_code_overrides() {
    // Guard: if someone moves PATH/CLAUDE_CODE_ENABLE_TASKS out of the managed
    // list, the user's shell PATH could clobber the augmented PATH at spawn.
    let managed = login_shell_env::managed_keys_for_test();
    assert!(managed.contains(&"PATH"));
    assert!(managed.contains(&"RUSTC"));
    assert!(managed.contains(&"RUSTUP_TOOLCHAIN"));
    assert!(managed.contains(&"CLAUDE_CODE_ENABLE_TASKS"));
    assert!(managed.contains(&"CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"));
    assert!(managed.contains(&"CLAUDE_PLUGIN_ROOT"));
    assert!(managed.contains(&"TAURI_API_URL"));
    assert!(managed.contains(&"DEBUG"));
}
