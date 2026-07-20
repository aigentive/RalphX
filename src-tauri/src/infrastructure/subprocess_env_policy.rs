//! Central environment policy for RalphX-managed child processes.

use std::sync::atomic::{AtomicBool, Ordering};

static REMOVE_INHERITED_GITHUB_CLI_TOKENS: AtomicBool = AtomicBool::new(true);

/// GitHub CLI token variables that override credentials stored by `gh auth login`.
pub(crate) const GITHUB_CLI_TOKEN_ENV_VARS: [&str; 4] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
];

/// Provider-owned credential policy applied to RalphX-managed subprocesses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProviderCredentialEnvPolicy {
    remove_inherited_github_cli_tokens: bool,
}

impl ProviderCredentialEnvPolicy {
    pub(crate) fn github_cli() -> Self {
        Self::github_cli_with_token_removal(remove_inherited_github_cli_tokens())
    }

    pub(crate) const fn github_cli_with_token_removal(enabled: bool) -> Self {
        Self {
            remove_inherited_github_cli_tokens: enabled,
        }
    }

    pub(crate) fn blocks_env_key(self, key: &str) -> bool {
        self.remove_inherited_github_cli_tokens && GITHUB_CLI_TOKEN_ENV_VARS.contains(&key)
    }

    pub(crate) fn apply_to_std_command(self, command: &mut std::process::Command) {
        if !self.remove_inherited_github_cli_tokens {
            return;
        }
        for key in GITHUB_CLI_TOKEN_ENV_VARS {
            command.env_remove(key);
        }
    }

    pub(crate) fn apply_to_tokio_command(self, command: &mut tokio::process::Command) {
        self.apply_to_std_command(command.as_std_mut());
    }

    pub(crate) fn apply_to_terminal_command(self, command: &mut portable_pty::CommandBuilder) {
        if !self.remove_inherited_github_cli_tokens {
            return;
        }
        for key in GITHUB_CLI_TOKEN_ENV_VARS {
            command.env_remove(key);
        }
    }
}

pub(crate) fn github_cli_env_policy() -> ProviderCredentialEnvPolicy {
    ProviderCredentialEnvPolicy::github_cli()
}

pub(crate) fn set_remove_inherited_github_cli_tokens(enabled: bool) {
    REMOVE_INHERITED_GITHUB_CLI_TOKENS.store(enabled, Ordering::SeqCst);
}

pub(crate) fn remove_inherited_github_cli_tokens() -> bool {
    REMOVE_INHERITED_GITHUB_CLI_TOKENS.load(Ordering::SeqCst)
}

pub(crate) fn is_github_cli_token_env_var(key: &str) -> bool {
    github_cli_env_policy().blocks_env_key(key)
}
