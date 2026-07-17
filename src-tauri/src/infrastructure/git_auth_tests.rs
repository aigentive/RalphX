use std::ffi::OsStr;
use std::path::Path;

use super::git_auth::{
    classify_gh_api_failure, http_status_code, is_valid_github_login,
    probe_github_connection_status,
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
