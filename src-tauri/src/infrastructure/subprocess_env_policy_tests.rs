use std::ffi::OsStr;

use super::subprocess_env_policy::{ProviderCredentialEnvPolicy, GITHUB_CLI_TOKEN_ENV_VARS};

fn assert_removed(command: &std::process::Command) {
    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        assert_eq!(
            command
                .get_envs()
                .find_map(|(candidate, value)| (candidate == OsStr::new(key)).then_some(value))
                .flatten(),
            None,
            "{key} must not be available to the child"
        );
    }
}

#[test]
fn github_cli_policy_classifies_only_cli_token_variables() {
    let policy = ProviderCredentialEnvPolicy::github_cli();

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        assert!(policy.blocks_env_key(key), "{key} must be blocked");
    }
    for key in ["GH_HOST", "OPENAI_API_KEY", "ANTHROPIC_API_KEY", "HOME"] {
        assert!(!policy.blocks_env_key(key), "{key} must remain available");
    }
}

#[test]
fn github_cli_policy_removes_tokens_from_std_and_tokio_commands() {
    let policy = ProviderCredentialEnvPolicy::github_cli();
    let mut std_command = std::process::Command::new("/usr/bin/env");
    let mut tokio_command = tokio::process::Command::new("/usr/bin/env");

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        std_command.env(key, "stale-secret");
        tokio_command.env(key, "stale-secret");
    }
    std_command.env("OPENAI_API_KEY", "provider-secret");
    tokio_command.env("OPENAI_API_KEY", "provider-secret");

    policy.apply_to_std_command(&mut std_command);
    policy.apply_to_tokio_command(&mut tokio_command);

    assert_removed(&std_command);
    assert_removed(tokio_command.as_std());
    assert_eq!(
        std_command
            .get_envs()
            .find_map(|(key, value)| (key == OsStr::new("OPENAI_API_KEY")).then_some(value))
            .flatten(),
        Some(OsStr::new("provider-secret"))
    );
}

#[test]
fn github_cli_policy_removes_tokens_from_terminal_commands() {
    let policy = ProviderCredentialEnvPolicy::github_cli();
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        command.env(key, "stale-secret");
    }
    command.env("ANTHROPIC_API_KEY", "provider-secret");

    policy.apply_to_terminal_command(&mut command);

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        assert_eq!(command.get_env(key), None, "{key} must be removed");
    }
    assert_eq!(
        command.get_env("ANTHROPIC_API_KEY"),
        Some(OsStr::new("provider-secret"))
    );
}

#[test]
fn github_cli_policy_preserves_inherited_tokens_when_removal_is_disabled() {
    let policy = ProviderCredentialEnvPolicy::github_cli_with_token_removal(false);
    let mut std_command = std::process::Command::new("/usr/bin/env");
    let mut tokio_command = tokio::process::Command::new("/usr/bin/env");
    let mut terminal_command = portable_pty::CommandBuilder::new("/bin/sh");

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        std_command.env(key, "shell-secret");
        tokio_command.env(key, "shell-secret");
        terminal_command.env(key, "shell-secret");
    }

    policy.apply_to_std_command(&mut std_command);
    policy.apply_to_tokio_command(&mut tokio_command);
    policy.apply_to_terminal_command(&mut terminal_command);

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        assert_eq!(
            std_command
                .get_envs()
                .find_map(|(candidate, value)| (candidate == OsStr::new(key)).then_some(value))
                .flatten(),
            Some(OsStr::new("shell-secret"))
        );
        assert_eq!(
            tokio_command
                .as_std()
                .get_envs()
                .find_map(|(candidate, value)| (candidate == OsStr::new(key)).then_some(value))
                .flatten(),
            Some(OsStr::new("shell-secret"))
        );
        assert_eq!(
            terminal_command.get_env(key),
            Some(OsStr::new("shell-secret"))
        );
    }
}
