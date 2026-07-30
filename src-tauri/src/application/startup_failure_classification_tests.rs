use super::startup_failure_classification::classify_app_state_construction_failure;
use super::startup_status::StartupFailureCode;
use crate::error::AppError;

#[test]
fn disk_exhaustion_gets_its_own_code_and_an_actionable_summary() {
    let failure = classify_app_state_construction_failure(&AppError::InsufficientDiskSpace {
        operation: "upgrading the chat timeline".to_string(),
        required_bytes: 16_400_000_000,
        available_bytes: 3_200_000_000,
    });

    assert_eq!(failure.code, StartupFailureCode::InsufficientDiskSpace);
    assert_eq!(
        failure.diagnostic_summary,
        "RalphX needs 16.4 GB free to finish upgrading the chat timeline, but only 3.2 GB is \
         available. Free up disk space, then try again."
    );
}

#[test]
fn disk_exhaustion_summary_scales_units_down_to_readable_sizes() {
    let failure = classify_app_state_construction_failure(&AppError::InsufficientDiskSpace {
        operation: "upgrading the chat timeline".to_string(),
        required_bytes: 4_500_000,
        available_bytes: 900,
    });

    assert!(
        failure.diagnostic_summary.contains("4.5 MB"),
        "expected megabytes, got: {}",
        failure.diagnostic_summary
    );
    assert!(
        failure.diagnostic_summary.contains("900 bytes"),
        "expected raw bytes, got: {}",
        failure.diagnostic_summary
    );
}

/// The failure text reaches the bootstrap screen verbatim, so a database error
/// must not leak SQL or paths into it.
#[test]
fn other_construction_failures_stay_generic() {
    for error in [
        AppError::Database("no such column: chat_message_blocks.kind".to_string()),
        AppError::Infrastructure("/Users/someone/Library/... is unreadable".to_string()),
    ] {
        let failure = classify_app_state_construction_failure(&error);

        assert_eq!(failure.code, StartupFailureCode::AppStateConstruction);
        assert_eq!(
            failure.diagnostic_summary,
            "RalphX could not open its local workspace."
        );
    }
}
