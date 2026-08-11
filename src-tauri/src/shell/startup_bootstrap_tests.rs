use super::startup_bootstrap::{cleanup_previous_launch_logs_when_enabled, create_file_log};

#[test]
fn file_log_setup_returns_an_error_instead_of_aborting_startup() {
    let temp_dir = tempfile::tempdir().unwrap();
    let blocking_file = temp_dir.path().join("not-a-directory");
    std::fs::write(&blocking_file, "occupied").unwrap();

    let result = create_file_log(&blocking_file, "ralphx.log");

    assert!(result.is_err());
}

#[test]
fn file_log_setup_creates_the_process_owned_directory_and_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().join("logs");

    let (path, _file) = create_file_log(&log_dir, "ralphx.log").unwrap();

    assert_eq!(path, log_dir.join("ralphx.log"));
    assert!(path.is_file());
}

#[test]
fn file_log_setup_never_truncates_an_existing_launch_log() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();
    let existing = log_dir.join("ralphx_2026-07-30_10-00-00.log");
    std::fs::write(&existing, "previous launch output").unwrap();

    let (path, _file) = create_file_log(&log_dir, "ralphx_2026-07-30_10-00-00.log").unwrap();

    assert_eq!(path, log_dir.join("ralphx_2026-07-30_10-00-00_1.log"));
    assert_eq!(
        std::fs::read_to_string(&existing).unwrap(),
        "previous launch output",
        "same-second relaunch must not truncate the earlier launch log"
    );
}

#[test]
fn disabled_file_logging_skips_previous_log_cleanup() {
    let log_dir = tempfile::tempdir().unwrap();
    let old_log = log_dir.path().join("ralphx_2026-07-30_10-00-00.log");
    std::fs::write(&old_log, "previous output").unwrap();

    let warnings = cleanup_previous_launch_logs_when_enabled(
        false,
        log_dir.path(),
        "ralphx_2026-07-30_11-00-00.log",
        0,
    );

    assert!(warnings.is_empty());
    assert!(old_log.exists());
}

#[test]
fn enabled_file_logging_delegates_to_cleanup_and_removes_old_logs() {
    let log_dir = tempfile::tempdir().unwrap();
    let old_log = log_dir.path().join("ralphx_2026-07-30_09-00-00.log");
    let current_log = log_dir.path().join("ralphx_2026-07-30_11-00-00.log");
    std::fs::write(&old_log, "old output").unwrap();
    std::fs::write(&current_log, "current output").unwrap();

    let warnings = cleanup_previous_launch_logs_when_enabled(
        true,
        log_dir.path(),
        "ralphx_2026-07-30_11-00-00.log",
        0,
    );

    assert!(warnings.is_empty());
    assert!(!old_log.exists(), "old log should be cleaned up");
    assert!(current_log.exists(), "current log must survive cleanup");
}

#[test]
fn file_log_collision_exhaustion_returns_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();

    let base = "ralphx_2026-07-30_12-00-00";
    std::fs::write(log_dir.join(format!("{base}.log")), "").unwrap();
    for attempt in 1..10u32 {
        std::fs::write(log_dir.join(format!("{base}_{attempt}.log")), "").unwrap();
    }

    let result = create_file_log(&log_dir, &format!("{base}.log"));

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
}

#[test]
fn file_log_without_log_extension_still_retries() {
    let temp_dir = tempfile::tempdir().unwrap();
    let log_dir = temp_dir.path().join("logs");
    std::fs::create_dir_all(&log_dir).unwrap();

    std::fs::write(log_dir.join("custom_name.log"), "occupied").unwrap();

    let (path, _file) = create_file_log(&log_dir, "custom_name.log").unwrap();

    assert_eq!(path, log_dir.join("custom_name_1.log"));
}
