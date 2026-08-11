use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use super::mcp_registration_repair::{
    remove_reserved_user_registration_for_test,
    remove_reserved_user_registration_with_env_for_test, ReservedMcpRepairFailureCode,
};

const TEST_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

#[test]
fn repair_failure_codes_have_stable_protocol_values() {
    assert_eq!(
        ReservedMcpRepairFailureCode::ConfigRead.to_string(),
        "config_read_failed"
    );
    assert_eq!(
        ReservedMcpRepairFailureCode::CommandFailed.to_string(),
        "command_failed"
    );
    assert_eq!(ReservedMcpRepairFailureCode::Timeout.to_string(), "timeout");
    assert_eq!(
        ReservedMcpRepairFailureCode::PostconditionFailed.to_string(),
        "postcondition_failed"
    );
}

fn write_reserved_registration(home: &Path) {
    fs::write(
        home.join(".claude.json"),
        serde_json::json!({
            "mcpServers": {"ralphx": {
                "type": "stdio",
                "command": "node",
                    "args": ["missing-or-user-shaped.js"],
                    "env": {"TOKEN": "definition-is-not-inspected"}
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
    write_reserved_registration(home.path());
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 0\n");

    let error = remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT)
        .await
        .unwrap_err();

    assert_eq!(error, ReservedMcpRepairFailureCode::PostconditionFailed);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn missing_legacy_registration_is_a_safe_noop_without_invoking_cli() {
    let home = tempfile::tempdir().unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let changed =
        remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT)
            .await
            .unwrap();

    assert!(!changed);
}

#[cfg(unix)]
#[tokio::test]
async fn unreadable_legacy_config_fails_before_invoking_cli() {
    let home = tempfile::tempdir().unwrap();
    fs::write(home.path().join(".claude.json"), "not json").unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let error = remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT)
        .await
        .unwrap_err();

    assert_eq!(error, ReservedMcpRepairFailureCode::ConfigRead);
}

#[cfg(unix)]
#[tokio::test]
async fn arbitrary_reserved_definition_is_removed_and_nonzero_without_removal_fails() {
    let home = tempfile::tempdir().unwrap();
    fs::write(
        home.path().join(".claude.json"),
        r#"{"mcpServers":{"ralphx":{"command":"user-owned"}}}"#,
    )
    .unwrap();
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nexit 7\n");

    let command_failed =
        remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT)
            .await
            .unwrap_err();
    assert_eq!(command_failed, ReservedMcpRepairFailureCode::CommandFailed);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn nonzero_exit_after_removal_is_settled_from_provider_state() {
    let home = tempfile::tempdir().unwrap();
    write_reserved_registration(home.path());
    let cli = home.path().join("fake-claude");
    write_executable(
        &cli,
        "#!/bin/sh\nprintf '{}' > \"$HOME/.claude.json\"\nexit 7\n",
    );

    let changed =
        remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT)
            .await
            .unwrap();

    assert!(changed);
}

#[cfg(unix)]
#[tokio::test]
async fn cleanup_pins_home_after_provider_environment_is_applied() {
    let home = tempfile::tempdir().unwrap();
    let conflicting_home = tempfile::tempdir().unwrap();
    write_reserved_registration(home.path());
    write_reserved_registration(conflicting_home.path());
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nprintf '{}' > \"$HOME/.claude.json\"\n");
    let provider_env = HashMap::from([(
        "HOME".to_string(),
        conflicting_home.path().to_string_lossy().into_owned(),
    )]);

    let changed = remove_reserved_user_registration_with_env_for_test(
        &cli,
        home.path(),
        &provider_env,
        TEST_COMMAND_TIMEOUT,
    )
    .await
    .unwrap();

    assert!(changed);
    assert!(
        fs::read_to_string(conflicting_home.path().join(".claude.json"))
            .unwrap()
            .contains("ralphx")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_after_removal_is_settled_from_provider_state() {
    let home = tempfile::tempdir().unwrap();
    write_reserved_registration(home.path());
    let cli = home.path().join("fake-claude");
    write_executable(
        &cli,
        "#!/bin/sh\nprintf '{}' > \"$HOME/.claude.json\"\nsleep 3\n",
    );

    let changed =
        remove_reserved_user_registration_for_test(&cli, home.path(), Duration::from_secs(1))
            .await
            .unwrap();

    assert!(changed);
}

#[cfg(unix)]
#[tokio::test]
async fn timeout_kills_cleanup_and_preserves_the_registration() {
    let home = tempfile::tempdir().unwrap();
    write_reserved_registration(home.path());
    let cli = home.path().join("fake-claude");
    write_executable(&cli, "#!/bin/sh\nsleep 2\n");

    let error =
        remove_reserved_user_registration_for_test(&cli, home.path(), Duration::from_millis(20))
            .await
            .unwrap_err();

    assert_eq!(error, ReservedMcpRepairFailureCode::Timeout);
    assert!(home.path().join(".claude.json").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn concurrent_callers_run_the_constant_removal_once() {
    let home = tempfile::tempdir().unwrap();
    write_reserved_registration(home.path());
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

    let first = remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT);
    let second =
        remove_reserved_user_registration_for_test(&cli, home.path(), TEST_COMMAND_TIMEOUT);
    let (first, second) = tokio::join!(first, second);

    assert!(first.is_ok());
    assert!(second.is_ok());
    assert_eq!(fs::read_to_string(marker).unwrap(), "x");
}
