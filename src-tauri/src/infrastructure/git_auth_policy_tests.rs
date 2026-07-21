use std::ffi::OsStr;

use super::git_auth::apply_git_subprocess_env;
use super::subprocess_env_policy::GITHUB_CLI_TOKEN_ENV_VARS;

#[test]
fn git_subprocess_environment_removes_github_tokens_and_preserves_runtime_env() {
    let mut command = tokio::process::Command::new("/usr/bin/env");
    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        command.env(key, "stale-secret");
    }
    command.env("OPENAI_API_KEY", "provider-secret");

    apply_git_subprocess_env(&mut command);

    for key in GITHUB_CLI_TOKEN_ENV_VARS {
        assert!(command
            .as_std()
            .get_envs()
            .all(|(candidate, value)| { candidate != OsStr::new(key) || value.is_none() }));
    }
    assert!(command.as_std().get_envs().any(|(key, value)| {
        key == OsStr::new("OPENAI_API_KEY") && value == Some(OsStr::new("provider-secret"))
    }));
    assert!(command.as_std().get_envs().any(|(key, value)| {
        key == OsStr::new("GIT_TERMINAL_PROMPT") && value == Some(OsStr::new("0"))
    }));
    assert!(command
        .as_std()
        .get_envs()
        .any(|(key, value)| { key == OsStr::new("PATH") && value.is_some() }));
}
