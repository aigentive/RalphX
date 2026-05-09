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
}

pub(crate) fn apply_git_subprocess_env(command: &mut Command) {
    command.envs(git_subprocess_env());
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
    let mut child = match Command::new(resolve_gh_cli_path())
        .args(["auth", "status"])
        .envs(git_subprocess_env())
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
    let fetch_url = read_origin_url(working_dir, &["remote", "get-url", "origin"]).await?;
    let push_url =
        match read_origin_url(working_dir, &["remote", "get-url", "--push", "origin"]).await {
            Ok(Some(url)) => Some(url),
            _ => fetch_url.clone(),
        };

    Ok(GitRemoteAuthConfig {
        fetch_url,
        push_url,
    })
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
        .map_err(|_| AppError::GitOperation("git remote get-url timed out".to_string()))?
        .map_err(|error| {
            AppError::GitOperation(format!("failed to inspect git remote: {error}"))
        })?;

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
