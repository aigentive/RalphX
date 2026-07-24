use super::startup_bootstrap::create_file_log;

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
