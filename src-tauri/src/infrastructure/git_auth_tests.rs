use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;

use super::git_auth::{
    classify_gh_api_failure, http_status_code, inspect_repository_capability,
    is_supported_github_remote, is_valid_github_login, probe_github_connection_status,
    probe_github_connection_status_with_timeout, repository_capability_from_origin_config,
    GitRemoteAuthConfig, RepositoryCapability,
};
use super::tool_paths::TEST_ENV_MUTEX;
use crate::domain::services::github_service::{
    GithubConnectionDiagnostic, GithubConnectionState, GithubConnectionStatus,
};

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
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

fn write_fake_gh(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake gh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake gh metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark fake gh executable");
    }
}

fn write_fake_git(path: &Path, body: &str) {
    std::fs::write(path, body).expect("write fake git");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path)
            .expect("fake git metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("mark fake git executable");
    }
}

async fn probe_with_fake_gh(script_body: &str) -> GithubConnectionStatus {
    let script_body = script_body.to_owned();

    tokio::task::spawn_blocking(move || {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let fake_gh = temp_dir.path().join("gh");
        write_fake_gh(&fake_gh, &script_body);
        let _path = EnvGuard::set_os("PATH", temp_dir.path().as_os_str());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        runtime.block_on(probe_github_connection_status())
    })
    .await
    .expect("probe task")
}

async fn probe_with_fake_gh_timeout(
    script_body: &str,
    timeout: std::time::Duration,
) -> GithubConnectionStatus {
    let script_body = script_body.to_owned();

    tokio::task::spawn_blocking(move || {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let fake_gh = temp_dir.path().join("gh");
        write_fake_gh(&fake_gh, &script_body);
        let _path = EnvGuard::set_os("PATH", temp_dir.path().as_os_str());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        runtime.block_on(probe_github_connection_status_with_timeout(timeout))
    })
    .await
    .expect("probe task")
}

async fn inspect_capability_with_fake_git(script_body: &str) -> RepositoryCapability {
    let script_body = script_body.to_owned();

    tokio::task::spawn_blocking(move || {
        let _lock = TEST_ENV_MUTEX.lock().expect("env mutex");
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let fake_git = temp_dir.path().join("git");
        write_fake_git(&fake_git, &script_body);
        let _path = EnvGuard::set_os("PATH", temp_dir.path().as_os_str());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        runtime.block_on(inspect_repository_capability(temp_dir.path()))
    })
    .await
    .expect("capability task")
}

#[tokio::test]
async fn startup_probe_timeout_does_not_use_generic_git_command_budget() {
    let started_at = std::time::Instant::now();
    let status = probe_with_fake_gh_timeout(
        "#!/bin/sh\nsleep 2\nexit 0\n",
        std::time::Duration::from_millis(20),
    )
    .await;

    assert!(started_at.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(status.state, GithubConnectionState::ProbeFailed);
    assert_eq!(status.diagnostic, Some(GithubConnectionDiagnostic::Timeout));
}

#[test]
fn gh_api_503_is_provider_unavailable_without_leaking_output() {
    let secret = "ghp_example_secret";
    let (state, diagnostic) =
        classify_gh_api_failure(&format!("HTTP 503: upstream failed with token {secret}"));

    assert_eq!(state, GithubConnectionState::ProviderUnavailable);
    assert_eq!(diagnostic, GithubConnectionDiagnostic::Http5xx);
    assert!(!format!("{diagnostic:?}").contains(secret));
}

#[test]
fn gh_api_401_is_credential_rejected() {
    let (state, diagnostic) = classify_gh_api_failure("HTTP 401: Bad credentials");

    assert_eq!(state, GithubConnectionState::CredentialRejected);
    assert_eq!(diagnostic, GithubConnectionDiagnostic::CredentialsRejected);
}

#[test]
fn gh_api_network_failure_is_provider_unavailable() {
    let (state, diagnostic) =
        classify_gh_api_failure("failed to connect to api.github.com: network is unreachable");

    assert_eq!(state, GithubConnectionState::ProviderUnavailable);
    assert_eq!(diagnostic, GithubConnectionDiagnostic::Network);
}

#[test]
fn unexpected_gh_api_failure_is_probe_failed() {
    let (state, diagnostic) = classify_gh_api_failure("unexpected response shape");

    assert_eq!(state, GithubConnectionState::ProbeFailed);
    assert_eq!(diagnostic, GithubConnectionDiagnostic::UnexpectedResponse);
}

#[test]
fn http_status_code_accepts_punctuation_and_rejects_non_http_numbers() {
    assert_eq!(http_status_code("request failed: HTTP 502."), Some(502));
    assert_eq!(http_status_code("retry 502 without protocol label"), None);
}

#[test]
fn github_login_validation_matches_github_account_rules() {
    assert!(is_valid_github_login("octo-user"));
    assert!(is_valid_github_login(&"a".repeat(39)));
    assert!(!is_valid_github_login(""));
    assert!(!is_valid_github_login("-octo"));
    assert!(!is_valid_github_login("octo-"));
    assert!(!is_valid_github_login("octo_user"));
    assert!(!is_valid_github_login(&"a".repeat(40)));
}

#[test]
fn repository_capability_uses_effective_push_url_and_only_accepts_github_com() {
    let local_only = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: None,
        push_url: None,
        github_https_credential_helper_configured: false,
    });
    assert_eq!(local_only, RepositoryCapability::LocalOnly);

    let github_https = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some("https://github.com/owner/repo.git".to_string()),
        push_url: Some("https://github.com/owner/repo.git".to_string()),
        github_https_credential_helper_configured: false,
    });
    assert!(matches!(github_https, RepositoryCapability::Github { .. }));

    let github_ssh = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some("https://gitlab.com/owner/repo.git".to_string()),
        push_url: Some("ssh://git@github.com/owner/repo.git".to_string()),
        github_https_credential_helper_configured: false,
    });
    assert!(matches!(github_ssh, RepositoryCapability::Github { .. }));

    let mixed = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some("https://github.com/owner/repo.git".to_string()),
        push_url: Some("git@gitlab.com:owner/repo.git".to_string()),
        github_https_credential_helper_configured: false,
    });
    assert!(matches!(mixed, RepositoryCapability::OtherRemote { .. }));
}

#[test]
fn repository_capability_falls_back_to_fetch_and_requires_exact_github_repository_paths() {
    let fetch_only = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some("git@github.com:owner/repository.git".to_string()),
        push_url: None,
        github_https_credential_helper_configured: false,
    });
    assert_eq!(
        fetch_only,
        RepositoryCapability::Github {
            fetch_url: Some("git@github.com:owner/repository.git".to_string()),
            push_url: "git@github.com:owner/repository.git".to_string(),
        }
    );

    for url in [
        "https://github.com/owner/repository.git",
        "git@github.com:owner/repository.git",
        "ssh://git@github.com/owner/repository.git",
    ] {
        assert!(is_supported_github_remote(url), "{url} must be supported");
    }
    for url in [
        "https://github.com/owner/repository/issues",
        "git@github.com:owner/repository/extra",
        "ssh://git@github.com/owner/repository/tree/main",
        "https://github.com/owner",
        "https://github.com/owner//repository",
    ] {
        assert!(!is_supported_github_remote(url), "{url} must be rejected");
    }
}

#[test]
fn repository_capability_serialization_strips_https_userinfo_and_redacts_tokens() {
    let raw_url = "https://automation:ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ@github.com/owner/repo.git";
    let capability = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some(raw_url.to_string()),
        push_url: Some(raw_url.to_string()),
        github_https_credential_helper_configured: false,
    });

    assert_eq!(
        capability,
        RepositoryCapability::Github {
            fetch_url: Some("https://github.com/owner/repo.git".to_string()),
            push_url: "https://github.com/owner/repo.git".to_string(),
        }
    );
    let serialized = serde_json::to_string(&capability).expect("capability serializes");
    assert!(!serialized.contains("automation"));
    assert!(!serialized.contains("ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
}

#[test]
fn repository_capability_serialization_strips_scheme_url_query_and_fragment_secrets() {
    let fetch_url = "https://gitlab.com/owner/fetch.git?access_token=fetch-secret#fetch-fragment";
    let push_url = "https://github.com/owner/repo.git?access_token=push-secret#push-fragment";
    let capability = repository_capability_from_origin_config(&GitRemoteAuthConfig {
        fetch_url: Some(fetch_url.to_string()),
        push_url: Some(push_url.to_string()),
        github_https_credential_helper_configured: false,
    });

    assert_eq!(
        capability,
        RepositoryCapability::OtherRemote {
            fetch_url: Some("https://gitlab.com/owner/fetch.git".to_string()),
            push_url: "https://github.com/owner/repo.git".to_string(),
        }
    );
    let serialized = serde_json::to_string(&capability).expect("capability serializes");
    for fragment in [
        "access_token",
        "fetch-secret",
        "push-secret",
        "fetch-fragment",
        "push-fragment",
    ] {
        assert!(!serialized.contains(fragment));
    }
}

#[test]
fn repository_capability_rejects_file_and_generic_remote_urls() {
    for url in [
        "file:///tmp/repo.git",
        "/tmp/repo.git",
        "https://gitlab.com/owner/repo.git",
        "git@gitlab.com:owner/repo.git",
    ] {
        let capability = repository_capability_from_origin_config(&GitRemoteAuthConfig {
            fetch_url: Some(url.to_string()),
            push_url: Some(url.to_string()),
            github_https_credential_helper_configured: false,
        });
        assert!(matches!(
            capability,
            RepositoryCapability::OtherRemote { .. }
        ));
    }
}

#[tokio::test]
async fn repository_capability_returns_typed_inspection_failure_for_invalid_config() {
    let repo = tempfile::tempdir().expect("temporary repository path");
    std::fs::create_dir(repo.path().join(".git")).expect("git metadata directory");
    std::fs::create_dir(repo.path().join(".git").join("config")).expect("invalid config directory");

    let capability = inspect_repository_capability(repo.path()).await;

    assert!(matches!(
        capability,
        RepositoryCapability::InspectionFailed { .. }
    ));
}

#[tokio::test]
async fn repository_capability_uses_git_effective_urls_for_included_push_rewrites() {
    let repo = tempfile::tempdir().expect("temporary repository path");
    let included = tempfile::tempdir().expect("included git config");
    let include_path = included.path().join("capability.gitconfig");
    std::process::Command::new("git")
        .args(["init", "--initial-branch", "main"])
        .current_dir(repo.path())
        .output()
        .expect("git init should run");
    std::process::Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/owner/repo.git",
        ])
        .current_dir(repo.path())
        .output()
        .expect("git remote add should run");
    std::fs::write(
        &include_path,
        "[url \"https://gitlab.com/rewritten/\"]\n\tpushInsteadOf = https://github.com/\n",
    )
    .expect("included config should write");
    std::process::Command::new("git")
        .args([
            "config",
            "include.path",
            include_path.to_str().expect("utf-8 path"),
        ])
        .current_dir(repo.path())
        .output()
        .expect("git config should run");

    let capability = inspect_repository_capability(repo.path()).await;

    assert!(matches!(
        capability,
        RepositoryCapability::OtherRemote { push_url, .. }
            if push_url == "https://gitlab.com/rewritten/owner/repo.git"
    ));
}

#[tokio::test]
async fn repository_capability_uses_push_only_origin_url() {
    let capability = inspect_capability_with_fake_git(
        r#"#!/bin/sh
if [ "$1" = "rev-parse" ]; then
  echo true
  exit 0
fi
if [ "$1" = "remote" ] && [ "$2" = "get-url" ] && [ "$3" = "--push" ]; then
  echo git@github.com:owner/repo.git
  exit 0
fi
if [ "$1" = "remote" ] && [ "$2" = "get-url" ]; then
  echo "error: No such remote 'origin'" >&2
  exit 2
fi
exit 1
"#,
    )
    .await;

    assert_eq!(
        capability,
        RepositoryCapability::Github {
            fetch_url: None,
            push_url: "git@github.com:owner/repo.git".to_string(),
        }
    );
}

#[tokio::test]
async fn repository_capability_fails_closed_when_origin_url_inspection_fails() {
    let capability = inspect_capability_with_fake_git(
        r#"#!/bin/sh
if [ "$1" = "rev-parse" ]; then
  echo true
  exit 0
fi
if [ "$1" = "remote" ] && [ "$2" = "get-url" ]; then
  echo "fatal: unable to read repository configuration" >&2
  exit 128
fi
exit 1
"#,
    )
    .await;

    assert!(matches!(
        capability,
        RepositoryCapability::InspectionFailed { .. }
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn repository_capability_rejects_symlinked_git_directory() {
    let repo = tempfile::tempdir().expect("temporary repository path");
    let outside = tempfile::tempdir().expect("external git metadata path");
    std::fs::write(
        outside.path().join("config"),
        "[remote \"origin\"]\n\turl = https://github.com/owner/repo.git\n",
    )
    .expect("external git config");
    symlink(outside.path(), repo.path().join(".git")).expect("symlinked git metadata");

    let capability = inspect_repository_capability(repo.path()).await;

    assert!(matches!(
        capability,
        RepositoryCapability::InspectionFailed { .. }
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn repository_capability_rejects_symlinked_config_escape() {
    let repo = tempfile::tempdir().expect("temporary repository path");
    let outside = tempfile::tempdir().expect("external git config path");
    let git_directory = repo.path().join(".git");
    std::fs::create_dir(&git_directory).expect("git metadata directory");
    let external_config = outside.path().join("config");
    std::fs::write(
        &external_config,
        "[remote \"origin\"]\n\turl = https://github.com/owner/repo.git\n",
    )
    .expect("external git config");
    symlink(&external_config, git_directory.join("config")).expect("symlinked git config");

    let capability = inspect_repository_capability(repo.path()).await;

    assert!(matches!(
        capability,
        RepositoryCapability::InspectionFailed { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn github_connection_probe_authenticates_valid_live_credential() {
    let status = probe_with_fake_gh(
        r#"#!/bin/sh
if [ "$1" = "auth" ]; then
  exit 0
fi
if [ "$1" = "api" ]; then
  printf 'octo-user\n'
  exit 0
fi
exit 2
"#,
    )
    .await;

    assert_eq!(
        status,
        GithubConnectionStatus::authenticated("github.com", "octo-user")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn github_connection_probe_keeps_missing_token_as_unauthenticated() {
    let status = probe_with_fake_gh(
        r#"#!/bin/sh
if [ "$1" = "auth" ]; then
  exit 1
fi
exit 2
"#,
    )
    .await;

    assert_eq!(status, GithubConnectionStatus::unauthenticated());
}

#[tokio::test(flavor = "current_thread")]
async fn github_connection_probe_distinguishes_rejected_credential() {
    let status = probe_with_fake_gh(
        r#"#!/bin/sh
if [ "$1" = "auth" ]; then
  exit 0
fi
if [ "$1" = "api" ]; then
  printf 'HTTP 401: Bad credentials\n' >&2
  exit 1
fi
exit 2
"#,
    )
    .await;

    assert_eq!(status, GithubConnectionStatus::credential_rejected());
}

#[tokio::test(flavor = "current_thread")]
async fn github_connection_probe_treats_provider_5xx_as_transient() {
    let status = probe_with_fake_gh(
        r#"#!/bin/sh
if [ "$1" = "auth" ]; then
  exit 0
fi
if [ "$1" = "api" ]; then
  printf 'HTTP 503: Service unavailable\n' >&2
  exit 1
fi
exit 2
"#,
    )
    .await;

    assert_eq!(
        status,
        GithubConnectionStatus::provider_unavailable(GithubConnectionDiagnostic::Http5xx)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn github_connection_probe_rejects_malformed_success_payload() {
    let status = probe_with_fake_gh(
        r#"#!/bin/sh
if [ "$1" = "auth" ]; then
  exit 0
fi
if [ "$1" = "api" ]; then
  printf -- '-bad-login\n'
  exit 0
fi
exit 2
"#,
    )
    .await;

    assert_eq!(
        status,
        GithubConnectionStatus::probe_failed(GithubConnectionDiagnostic::MalformedResponse)
    );
}
