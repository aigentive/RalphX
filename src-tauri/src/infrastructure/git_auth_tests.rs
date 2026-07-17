use super::git_auth::classify_gh_api_failure;
use crate::domain::services::github_service::{GithubConnectionDiagnostic, GithubConnectionState};

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
