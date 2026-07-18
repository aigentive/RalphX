use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use crate::error::{AppError, AppResult};
use crate::infrastructure::tool_paths::{resolve_gh_cli_path, resolve_git_cli_path};
use crate::utils::path_safety::validate_absolute_non_root_path;
use crate::utils::secret_redactor::redact;

const GUI_SAFE_PATH_ENTRIES: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    "/bin",
    "/usr/sbin",
    "/sbin",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitRemoteUrlKind {
    Https,
    Ssh,
    File,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GitNetworkOperation {
    Fetch,
    Push,
    DeleteRemoteBranch,
}

impl GitNetworkOperation {
    pub(crate) fn from_args(args: &[String]) -> Option<Self> {
        match args.first().map(String::as_str) {
            Some("fetch") => Some(Self::Fetch),
            Some("push") if args.iter().any(|arg| arg == "--delete") => {
                Some(Self::DeleteRemoteBranch)
            }
            Some("push") => Some(Self::Push),
            _ => None,
        }
    }

    fn verb(self) -> &'static str {
        match self {
            Self::Fetch => "fetch from",
            Self::Push => "push to",
            Self::DeleteRemoteBranch => "delete a branch from",
        }
    }

    fn remote_label(self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Push | Self::DeleteRemoteBranch => "push",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GitRemoteAuthConfig {
    pub fetch_url: Option<String>,
    pub push_url: Option<String>,
    pub github_https_credential_helper_configured: bool,
}

impl GitRemoteAuthConfig {
    pub(crate) fn url_for_operation(&self, operation: GitNetworkOperation) -> Option<&str> {
        match operation {
            GitNetworkOperation::Fetch => self.fetch_url.as_deref(),
            GitNetworkOperation::Push | GitNetworkOperation::DeleteRemoteBranch => {
                self.push_url.as_deref().or(self.fetch_url.as_deref())
            }
        }
    }

    pub(crate) fn fetch_kind(&self) -> Option<GitRemoteUrlKind> {
        self.fetch_url.as_deref().map(classify_git_remote_url)
    }

    pub(crate) fn push_kind(&self) -> Option<GitRemoteUrlKind> {
        self.push_url
            .as_deref()
            .or(self.fetch_url.as_deref())
            .map(classify_git_remote_url)
    }

    pub(crate) fn has_mixed_auth_modes(&self) -> bool {
        let fetch_kind = self.fetch_kind();
        let push_kind = self.push_kind();
        fetch_kind.is_some() && push_kind.is_some() && fetch_kind != push_kind
    }

    pub(crate) fn has_github_https_remote(&self) -> bool {
        [self.fetch_url.as_deref(), self.push_url.as_deref()]
            .into_iter()
            .flatten()
            .any(is_github_https_remote)
    }
}

pub(crate) fn apply_git_subprocess_env(command: &mut Command) {
    command.envs(git_subprocess_env());
    crate::infrastructure::subprocess_env_policy::github_cli_env_policy()
        .apply_to_tokio_command(command);
}

pub(crate) fn git_subprocess_env() -> Vec<(String, String)> {
    let mut env = vec![
        ("GIT_TERMINAL_PROMPT".to_string(), "0".to_string()),
        ("PATH".to_string(), gui_safe_path()),
    ];

    if let Ok(home) = std::env::var("HOME") {
        env.push(("HOME".to_string(), home));
    }
    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        env.push(("SSH_AUTH_SOCK".to_string(), sock));
    }

    env
}

pub(crate) fn classify_git_remote_url(url: &str) -> GitRemoteUrlKind {
    let trimmed = url.trim();
    if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        GitRemoteUrlKind::Https
    } else if trimmed.starts_with("git@") || trimmed.starts_with("ssh://") {
        GitRemoteUrlKind::Ssh
    } else if trimmed.starts_with("file://")
        || trimmed.starts_with('/')
        || trimmed.starts_with("./")
        || trimmed.starts_with("../")
    {
        GitRemoteUrlKind::File
    } else {
        GitRemoteUrlKind::Other
    }
}

pub(crate) fn git_remote_url_kind_label(kind: Option<GitRemoteUrlKind>) -> &'static str {
    kind_label(kind)
}

pub(crate) async fn check_gh_auth_status() -> bool {
    let mut command = Command::new(resolve_gh_cli_path());
    apply_git_subprocess_env(&mut command);
    let mut child = match command
        .args(["auth", "status"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    match timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => status.success(),
        _ => false,
    }
}

pub(crate) async fn check_gh_auth_token_available() -> bool {
    let mut command = Command::new(resolve_gh_cli_path());
    apply_git_subprocess_env(&mut command);
    let child = match command
        .args(gh_auth_token_args())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    match timeout(Duration::from_secs(2), child.wait_with_output()).await {
        Ok(Ok(output)) => {
            output.status.success() && output.stdout.iter().any(|byte| !byte.is_ascii_whitespace())
        }
        _ => false,
    }
}

fn gh_auth_token_args() -> [&'static str; 4] {
    ["auth", "token", "--hostname", "github.com"]
}

pub(crate) fn github_https_remote_to_ssh(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let path = trimmed.strip_prefix("https://github.com/")?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some()
        || !is_safe_github_path_component(owner)
        || !is_safe_github_path_component(repo)
    {
        return None;
    }

    Some(format!("git@github.com:{owner}/{repo}.git"))
}

pub(crate) fn suggested_github_ssh_origin(config: &GitRemoteAuthConfig) -> Option<String> {
    config
        .fetch_url
        .as_deref()
        .and_then(github_https_remote_to_ssh)
        .or_else(|| {
            config
                .push_url
                .as_deref()
                .and_then(github_https_remote_to_ssh)
        })
}

pub(crate) fn is_git_auth_failure_text(text: &str) -> bool {
    let normalized = text.to_lowercase();
    const PATTERNS: &[&str] = &[
        "could not read username",
        "terminal prompts disabled",
        "device not configured",
        "authentication failed",
        "permission denied (publickey)",
        "host key verification failed",
        "could not read from remote repository",
        "support for password authentication was removed",
    ];

    PATTERNS.iter().any(|pattern| normalized.contains(pattern))
}

pub(crate) async fn git_auth_error_from_failure(
    operation: GitNetworkOperation,
    working_dir: &Path,
    stderr: &str,
) -> Option<AppError> {
    if !is_git_auth_failure_text(stderr) {
        return None;
    }

    let remotes = inspect_origin_auth_config(working_dir).await.ok();
    Some(AppError::GitAuth(format_git_auth_recovery(
        operation,
        remotes.as_ref(),
        stderr,
    )))
}

pub(crate) async fn inspect_origin_auth_config(
    working_dir: &Path,
) -> AppResult<GitRemoteAuthConfig> {
    let mut config = if let Some(config) = read_origin_auth_config_from_git_config(working_dir)? {
        config
    } else {
        let fetch_url = read_origin_url(working_dir, &["remote", "get-url", "origin"]).await?;
        let push_url = if fetch_url.is_some() {
            match read_origin_url(working_dir, &["remote", "get-url", "--push", "origin"]).await {
                Ok(Some(url)) => Some(url),
                _ => fetch_url.clone(),
            }
        } else {
            None
        };

        GitRemoteAuthConfig {
            fetch_url,
            push_url,
            github_https_credential_helper_configured: false,
        }
    };

    config.github_https_credential_helper_configured =
        inspect_github_https_credential_helper_configured(working_dir, &config).await;

    Ok(config)
}

fn read_origin_auth_config_from_git_config(
    working_dir: &Path,
) -> AppResult<Option<GitRemoteAuthConfig>> {
    let working_dir = validate_absolute_non_root_path(working_dir, "git working directory")?;
    let config_path = working_dir.join(".git").join("config");
    if !config_path.is_file() {
        return Ok(None);
    }

    let started_at = std::time::Instant::now();
    let raw = std::fs::read_to_string(&config_path).map_err(|error| {
        AppError::GitOperation(format!(
            "failed to read git config for origin inspection: {error}"
        ))
    })?;
    let elapsed_ms = started_at.elapsed().as_millis();
    if elapsed_ms >= 500 {
        tracing::warn!(
            cwd = %working_dir.display(),
            elapsed_ms,
            "Startup Git auth preflight: slow git config file read completed"
        );
    } else {
        tracing::debug!(
            cwd = %working_dir.display(),
            elapsed_ms,
            "Startup Git auth preflight: git config file read completed"
        );
    }

    Ok(Some(parse_origin_auth_config_from_git_config(&raw)))
}

fn parse_origin_auth_config_from_git_config(raw: &str) -> GitRemoteAuthConfig {
    let mut in_origin_remote = false;
    let mut fetch_url = None;
    let mut push_url = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_origin_remote = is_origin_remote_section(trimmed);
            continue;
        }

        if !in_origin_remote {
            continue;
        }

        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim().trim_matches('"').to_string();
        if value.is_empty() {
            continue;
        }

        if key.eq_ignore_ascii_case("url") && fetch_url.is_none() {
            fetch_url = Some(value);
        } else if key.eq_ignore_ascii_case("pushurl") && push_url.is_none() {
            push_url = Some(value);
        }
    }

    if push_url.is_none() {
        push_url = fetch_url.clone();
    }

    GitRemoteAuthConfig {
        fetch_url,
        push_url,
        github_https_credential_helper_configured: false,
    }
}

fn is_origin_remote_section(section: &str) -> bool {
    let body = section
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();
    body.eq_ignore_ascii_case(r#"remote "origin""#)
        || body.eq_ignore_ascii_case("remote.origin")
        || body.eq_ignore_ascii_case("remote 'origin'")
}

async fn inspect_github_https_credential_helper_configured(
    working_dir: &Path,
    config: &GitRemoteAuthConfig,
) -> bool {
    if !config.has_github_https_remote() {
        return false;
    }

    let Ok(working_dir) = validate_absolute_non_root_path(working_dir, "git working directory")
    else {
        return false;
    };
    let started_at = std::time::Instant::now();
    let mut command = Command::new(resolve_git_cli_path());
    apply_git_subprocess_env(&mut command);
    let child = match command
        .args([
            "config",
            "--get-urlmatch",
            "credential.helper",
            "https://github.com",
        ])
        .current_dir(&working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            tracing::warn!(
                cwd = %working_dir.display(),
                error = %error,
                "Git auth diagnostics: failed to spawn credential helper inspection"
            );
            return false;
        }
    };

    let output = match timeout(Duration::from_secs(5), child.wait_with_output()).await {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            tracing::warn!(
                cwd = %working_dir.display(),
                error = %error,
                "Git auth diagnostics: failed to inspect credential helper"
            );
            return false;
        }
        Err(_) => {
            tracing::warn!(
                cwd = %working_dir.display(),
                elapsed_ms = started_at.elapsed().as_millis(),
                "Git auth diagnostics: credential helper inspection timed out"
            );
            return false;
        }
    };

    output.status.success() && credential_helper_output_has_configured_helper(&output.stdout)
}

fn credential_helper_output_has_configured_helper(stdout: &[u8]) -> bool {
    String::from_utf8_lossy(stdout)
        .lines()
        .any(|line| !line.trim().is_empty())
}

fn format_git_auth_recovery(
    operation: GitNetworkOperation,
    remotes: Option<&GitRemoteAuthConfig>,
    stderr: &str,
) -> String {
    let mut parts = vec![format!(
        "Git could not authenticate while trying to {} `origin`.",
        operation.verb()
    )];

    if let Some(remotes) = remotes {
        let target_kind = remotes
            .url_for_operation(operation)
            .map(classify_git_remote_url);
        let fetch_kind = remotes.fetch_kind();
        let push_kind = remotes.push_kind();

        match target_kind {
            Some(GitRemoteUrlKind::Https) => {
                parts.push(format!(
                    "The {} remote uses HTTPS, so SSH keys are not used for this operation.",
                    operation.remote_label()
                ));
                parts.push(
                    "Configure a non-interactive Git credential helper/token, run `gh auth setup-git` for GitHub HTTPS remotes, or switch the remote URL to SSH."
                        .to_string(),
                );
            }
            Some(GitRemoteUrlKind::Ssh) => {
                parts.push(format!(
                    "The {} remote uses SSH, but RalphX could not access an SSH key from this process.",
                    operation.remote_label()
                ));
                if std::env::var_os("SSH_AUTH_SOCK").is_none() {
                    parts.push("`SSH_AUTH_SOCK` is not set for the RalphX process.".to_string());
                }
                parts.push(
                    "Add the key to a macOS keychain-backed SSH agent or configure this repo to use HTTPS credentials."
                        .to_string(),
                );
            }
            _ => {
                parts.push(
                    "Configure credentials for the repository remote, or update `origin` to an authenticated HTTPS or SSH URL."
                        .to_string(),
                );
            }
        }

        if fetch_kind.is_some() && push_kind.is_some() && fetch_kind != push_kind {
            parts.push(format!(
                "Remote auth modes are mixed: fetch uses {}, push uses {}.",
                kind_label(fetch_kind),
                kind_label(push_kind)
            ));
        }
    } else {
        parts.push(
            "RalphX could not inspect `origin`; configure the repository remote credentials and retry."
                .to_string(),
        );
    }

    let stderr = redact(stderr).trim().to_string();
    if !stderr.is_empty() {
        parts.push(format!("Git reported: {stderr}"));
    }

    parts.join(" ")
}

async fn read_origin_url(working_dir: &Path, args: &[&str]) -> AppResult<Option<String>> {
    let working_dir = validate_absolute_non_root_path(working_dir, "git working directory")?;
    let started_at = std::time::Instant::now();
    let mut command = Command::new(resolve_git_cli_path());
    apply_git_subprocess_env(&mut command);
    let child = command
        .args(args)
        .current_dir(&working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| AppError::GitOperation(format!("failed to spawn git: {error}")))?;

    let output = timeout(Duration::from_secs(5), child.wait_with_output())
        .await
        .map_err(|_| {
            tracing::warn!(
                command = %args.join(" "),
                cwd = %working_dir.display(),
                elapsed_ms = started_at.elapsed().as_millis(),
                "Startup Git auth preflight: git remote inspection timed out"
            );
            AppError::GitOperation("git remote get-url timed out".to_string())
        })?
        .map_err(|error| {
            AppError::GitOperation(format!("failed to inspect git remote: {error}"))
        })?;
    let elapsed_ms = started_at.elapsed().as_millis();
    if elapsed_ms >= 500 {
        tracing::warn!(
            command = %args.join(" "),
            cwd = %working_dir.display(),
            elapsed_ms,
            success = output.status.success(),
            "Startup Git auth preflight: slow git remote inspection completed"
        );
    }

    if !output.status.success() {
        return Ok(None);
    }

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!url.is_empty()).then_some(url))
}

fn gui_safe_path() -> String {
    let mut entries = Vec::new();
    if let Ok(path) = std::env::var("PATH") {
        entries.extend(
            path.split(':')
                .filter(|entry| !entry.is_empty())
                .map(str::to_string),
        );
    }
    for entry in GUI_SAFE_PATH_ENTRIES {
        if !entries.iter().any(|existing| existing == entry) {
            entries.push((*entry).to_string());
        }
    }
    entries.join(":")
}

fn is_safe_github_path_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn is_github_https_remote(url: &str) -> bool {
    url.trim().starts_with("https://github.com/")
        && matches!(classify_git_remote_url(url), GitRemoteUrlKind::Https)
}

fn kind_label(kind: Option<GitRemoteUrlKind>) -> &'static str {
    match kind {
        Some(GitRemoteUrlKind::Https) => "HTTPS",
        Some(GitRemoteUrlKind::Ssh) => "SSH",
        Some(GitRemoteUrlKind::File) => "file",
        Some(GitRemoteUrlKind::Other) => "other",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    #[test]
    fn classifies_remote_url_kinds() {
        assert_eq!(
            classify_git_remote_url("https://github.com/owner/repo.git"),
            GitRemoteUrlKind::Https
        );
        assert_eq!(
            classify_git_remote_url("git@github.com:owner/repo.git"),
            GitRemoteUrlKind::Ssh
        );
        assert_eq!(
            classify_git_remote_url("ssh://git@github.com/owner/repo.git"),
            GitRemoteUrlKind::Ssh
        );
        assert_eq!(
            classify_git_remote_url("file:///tmp/repo.git"),
            GitRemoteUrlKind::File
        );
        assert_eq!(
            classify_git_remote_url("/tmp/repo.git"),
            GitRemoteUrlKind::File
        );
    }

    #[test]
    fn detects_common_auth_failures() {
        assert!(is_git_auth_failure_text(
            "fatal: could not read Username for 'https://github.com': Device not configured"
        ));
        assert!(is_git_auth_failure_text(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled"
        ));
        assert!(is_git_auth_failure_text(
            "git@github.com: Permission denied (publickey)."
        ));
        assert!(is_git_auth_failure_text("Host key verification failed."));
        assert!(!is_git_auth_failure_text(
            "failed to push some refs: non-fast-forward"
        ));
    }

    #[test]
    fn git_subprocess_env_disables_prompts_and_has_gui_safe_path() {
        let env = git_subprocess_env();
        assert!(env
            .iter()
            .any(|(key, value)| key == "GIT_TERMINAL_PROMPT" && value == "0"));
        let path = env
            .iter()
            .find_map(|(key, value)| (key == "PATH").then_some(value.as_str()))
            .expect("PATH should be set");
        assert!(path.split(':').any(|entry| entry == "/usr/bin"));
        assert!(path.split(':').any(|entry| entry == "/bin"));
    }

    #[test]
    fn recovery_message_explains_mixed_https_fetch_ssh_push() {
        let remotes = GitRemoteAuthConfig {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: Some("git@github.com:owner/repo.git".to_string()),
            github_https_credential_helper_configured: false,
        };

        let message = format_git_auth_recovery(
            GitNetworkOperation::Fetch,
            Some(&remotes),
            "fatal: could not read Username for 'https://github.com': Device not configured",
        );

        assert!(message.contains("fetch remote uses HTTPS"));
        assert!(message.contains("SSH keys are not used"));
        assert!(message.contains("fetch uses HTTPS, push uses SSH"));
        assert!(message.contains("gh auth setup-git"));
    }

    #[test]
    fn converts_github_https_remote_to_ssh() {
        assert_eq!(
            github_https_remote_to_ssh("https://github.com/owner/repo.git"),
            Some("git@github.com:owner/repo.git".to_string())
        );
        assert_eq!(
            github_https_remote_to_ssh("https://github.com/owner/repo"),
            Some("git@github.com:owner/repo.git".to_string())
        );
        assert_eq!(
            github_https_remote_to_ssh("https://github.com/owner/repo/extra"),
            None
        );
        assert_eq!(
            github_https_remote_to_ssh("https://example.com/owner/repo.git"),
            None
        );
    }

    #[test]
    fn derives_network_operation_from_git_args() {
        assert_eq!(
            GitNetworkOperation::from_args(&["fetch".to_string(), "origin".to_string()]),
            Some(GitNetworkOperation::Fetch)
        );
        assert_eq!(
            GitNetworkOperation::from_args(&["push".to_string(), "origin".to_string()]),
            Some(GitNetworkOperation::Push)
        );
        assert_eq!(
            GitNetworkOperation::from_args(&[
                "push".to_string(),
                "origin".to_string(),
                "--delete".to_string(),
                "branch".to_string()
            ]),
            Some(GitNetworkOperation::DeleteRemoteBranch)
        );
    }

    #[test]
    fn gh_token_probe_targets_github_without_status_network_check() {
        assert_eq!(
            gh_auth_token_args(),
            ["auth", "token", "--hostname", "github.com"]
        );
    }

    #[test]
    fn parses_origin_remote_from_git_config_without_git_subprocess() {
        let raw = r#"
            [core]
                repositoryformatversion = 0
            [remote "origin"]
                url = https://github.com/owner/repo.git
                pushurl = git@github.com:owner/repo.git
            [branch "main"]
                remote = origin
        "#;

        let config = parse_origin_auth_config_from_git_config(raw);

        assert_eq!(
            config.fetch_url.as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(
            config.push_url.as_deref(),
            Some("git@github.com:owner/repo.git")
        );
    }

    #[test]
    fn parses_origin_remote_push_url_from_fetch_url_when_missing() {
        let raw = r#"
            [remote "origin"]
                url = https://github.com/owner/repo.git
        "#;

        let config = parse_origin_auth_config_from_git_config(raw);

        assert_eq!(
            config.fetch_url.as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(config.push_url, config.fetch_url);
        assert!(!config.github_https_credential_helper_configured);
    }

    #[test]
    fn detects_non_empty_credential_helper_output() {
        assert!(credential_helper_output_has_configured_helper(
            b"\n!gh auth git-credential\n"
        ));
        assert!(!credential_helper_output_has_configured_helper(b"\n \n"));
    }

    #[tokio::test]
    async fn inspect_origin_auth_config_detects_github_https_credential_helper() {
        let repo = tempfile::tempdir().expect("temp repo");
        git(repo.path(), &["init"]);
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/repo.git",
            ],
        );
        git(
            repo.path(),
            &[
                "config",
                "credential.https://github.com.helper",
                "!gh auth git-credential",
            ],
        );

        let config = inspect_origin_auth_config(repo.path())
            .await
            .expect("origin config should inspect");

        assert!(config.github_https_credential_helper_configured);
    }

    #[test]
    fn parses_origin_remote_section_aliases_and_ignores_other_remotes() {
        let dot_section = r#"
            [remote.upstream]
                url = https://github.com/other/repo.git
            [remote.origin]
                url = git@github.com:owner/repo.git
        "#;
        let single_quote_section = r#"
            [remote 'origin']
                url = https://github.com/owner/repo.git
        "#;

        assert_eq!(
            parse_origin_auth_config_from_git_config(dot_section)
                .fetch_url
                .as_deref(),
            Some("git@github.com:owner/repo.git")
        );
        assert_eq!(
            parse_origin_auth_config_from_git_config(single_quote_section)
                .fetch_url
                .as_deref(),
            Some("https://github.com/owner/repo.git")
        );
    }

    #[test]
    fn reads_origin_remote_from_git_config_file_without_git_subprocess() {
        let repo = tempfile::tempdir().expect("temp repo");
        let git_dir = repo.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("git dir");
        std::fs::write(
            git_dir.join("config"),
            r#"
                [core]
                    repositoryformatversion = 0
                [remote "origin"]
                    url = https://github.com/owner/repo.git
                    pushurl = git@github.com:owner/repo.git
            "#,
        )
        .expect("write git config");

        let config = read_origin_auth_config_from_git_config(repo.path())
            .expect("config read should succeed")
            .expect("origin config should be read");

        assert_eq!(
            config.fetch_url.as_deref(),
            Some("https://github.com/owner/repo.git")
        );
        assert_eq!(
            config.push_url.as_deref(),
            Some("git@github.com:owner/repo.git")
        );
    }

    #[test]
    fn missing_git_config_falls_back_to_git_remote_inspection() {
        let repo = tempfile::tempdir().expect("temp repo");

        assert!(read_origin_auth_config_from_git_config(repo.path())
            .expect("missing config should not error")
            .is_none());
    }

    #[tokio::test]
    async fn auth_error_from_failure_hydrates_mixed_origin_urls() {
        let repo = tempfile::tempdir().expect("temp repo");
        git(repo.path(), &["init"]);
        git(
            repo.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/owner/repo.git",
            ],
        );
        git(
            repo.path(),
            &[
                "remote",
                "set-url",
                "--push",
                "--add",
                "origin",
                "git@github.com:owner/repo.git",
            ],
        );

        let error = git_auth_error_from_failure(
            GitNetworkOperation::Fetch,
            repo.path(),
            "fatal: could not read Username for 'https://github.com': Device not configured",
        )
        .await
        .expect("auth failure should classify");

        let AppError::GitAuth(message) = error else {
            panic!("expected GitAuth");
        };
        assert!(message.contains("fetch remote uses HTTPS"));
        assert!(message.contains("fetch uses HTTPS, push uses SSH"));
    }

    fn git(repo: &Path, args: &[&str]) {
        let output = StdCommand::new(resolve_git_cli_path())
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git should spawn");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
