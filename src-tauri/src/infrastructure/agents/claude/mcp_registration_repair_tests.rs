use std::fs;
use std::path::Path;
use std::time::Duration;

use super::mcp_registration_repair::{
    retire_exact_legacy_user_registration_for_test, LegacyMcpRepairFailureCode,
};

#[test]
fn repair_failure_codes_have_stable_protocol_values() {
    assert_eq!(
        LegacyMcpRepairFailureCode::ConfigRead.to_string(),
        "config_read_failed"
    );
    assert_eq!(
        LegacyMcpRepairFailureCode::NoExactHistoricalMatch.to_string(),
        "no_exact_historical_match"
    );
    assert_eq!(
        LegacyMcpRepairFailureCode::CommandFailed.to_string(),
        "command_failed"
    );
    assert_eq!(LegacyMcpRepairFailureCode::Timeout.to_string(), "timeout");
    assert_eq!(
        LegacyMcpRepairFailureCode::PostconditionFailed.to_string(),
        "postcondition_failed"
    );
}

fn write_exact_registration(home: &Path, app_data: &Path) {
    let script = app_data.join("generated/release/claude-plugin/ralphx-mcp-server/build/index.js");
    fs::create_dir_all(script.parent().unwrap()).unwrap();
    fs::write(&script, "fixture").unwrap();
    fs::write(
        home.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {"ralphx": {
                "type": "stdio",
                "command": "node",
                "args": [
                    script,
                    "--trace-dir",
                    app_data.join("logs/mcp-proxy")
                ]
            }}
        })
        .to_string(),
    )
    .unwrap();
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn successful_cli_without_absence_fails_the_postcondition() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    write_exact_registration(home.path(), app_data.path());
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 0\n");

    let error = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();

    assert_eq!(error, LegacyMcpRepairFailureCode::PostconditionFailed);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn missing_legacy_registration_is_a_safe_noop_without_invoking_cli() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let changed = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap();

    assert!(!changed);
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_legacy_config_fails_before_invoking_cli() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    fs::write(home.path().join(".claude.json"), "not json").unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let error = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();

    assert_eq!(error, LegacyMcpRepairFailureCode::ConfigRead);
}

#[cfg(unix)]
#[tokio::test]
async fn ambiguous_and_failed_cleanup_are_distinguished_without_mutating_config() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"ralphx":{"command":"user-owned"}}}"#,
    )
    .unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let ambiguous = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert_eq!(
        ambiguous,
        LegacyMcpRepairFailureCode::NoExactHistoricalMatch
    );

    write_exact_registration(home.path(), app_data.path());
    let command_failed = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert_eq!(command_failed, LegacyMcpRepairFailureCode::CommandFailed);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_kills_cleanup_and_preserves_the_registration() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    write_exact_registration(home.path(), app_data.path());
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nsleep 2\n");

    let error = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_millis(20),
    )
    .await
    .unwrap_err();

    assert_eq!(error, LegacyMcpRepairFailureCode::Timeout);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_callers_run_the_constant_removal_once() {
    let home = tempfile::tempdir().unwrap();
    let app_data = tempfile::tempdir().unwrap();
    write_exact_registration(home.path(), app_data.path());
    let config = home.path().join(".claude.json");
    let marker = home.path().join("cleanup-count");
    let cli = home.path().join("fake-claude");
    write_executable(
        &cli,
        &format!(
            "#!/bin/sh\n[ \"$1 $2 $3 $4 $5\" = \"mcp remove ralphx -s user\" ] || exit 9\nprintf '{{}}' > '{}'\nprintf x >> '{}'\n",
            config.display(),
            marker.display(),
        ),
    );

    let first = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    );
    let second = retire_exact_legacy_user_registration_for_test(
        &cli,
        home.path(),
        app_data.path(),
        Duration::from_secs(1),
    );
    let (first, second) = tokio::join!(first, second);

    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
}
