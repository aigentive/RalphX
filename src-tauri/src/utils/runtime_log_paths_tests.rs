use super::*;

#[test]
fn merge_validation_log_dir_hashes_task_id_components() {
    let path = merge_validation_log_dir("../task/with\\separators");
    let suffix = path
        .file_name()
        .and_then(|value| value.to_str())
        .expect("log dir suffix");

    assert!(path.starts_with(app_log_dir().join("merge-validation")));
    assert!(suffix.starts_with("task-"));
    assert!(!suffix.contains(".."));
    assert!(!suffix.contains('/'));
    assert!(!suffix.contains('\\'));
}

#[test]
fn codex_prompt_debug_file_maps_unknown_modes_to_fixed_component() {
    let path = codex_prompt_debug_file("../resume");
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .expect("prompt debug filename");

    assert!(path.starts_with(codex_prompt_debug_dir()));
    assert!(filename.contains("-unknown-"));
    assert!(!filename.contains(".."));
    assert!(!filename.contains('/'));
    assert!(!filename.contains('\\'));
}

#[test]
fn stream_debug_log_file_hashes_conversation_id() {
    let path = stream_debug_log_file("../conversation/with\\separators");
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .expect("stream debug filename");

    assert!(path.starts_with(app_log_dir().join("stream-debug")));
    assert!(filename.starts_with("conversation-"));
    assert!(filename.ends_with(".log"));
    assert!(!filename.contains(".."));
    assert!(!filename.contains('/'));
    assert!(!filename.contains('\\'));
}

#[test]
fn ensure_mcp_proxy_trace_dir_creates_app_owned_dir() {
    let dir = ensure_mcp_proxy_trace_dir();

    assert!(dir.starts_with(app_log_dir()));
    assert_eq!(
        dir.file_name().and_then(|value| value.to_str()),
        Some("mcp-proxy")
    );
    assert!(dir.is_dir());
}

#[test]
fn memory_archive_paths_hash_runtime_components() {
    let project_dir = memory_archive_project_dir("../project/with\\separators");
    let project_relative_dir = memory_archive_project_relative_dir("../project/with\\separators");
    let memory_path =
        memory_archive_memory_snapshot_file("../project/with\\separators", "../memory/unsafe.md");
    let memory_relative_path = memory_archive_memory_snapshot_relative_file(
        "../project/with\\separators",
        "../memory/unsafe.md",
    );
    let rule_path = memory_archive_rule_snapshot_file(
        "../project/with\\separators",
        "../rules/unsafe.md",
        "20260516_120000",
    );
    let project_path =
        memory_archive_project_snapshot_file("../project/with\\separators", "../bad");
    let project_relative_path =
        memory_archive_project_snapshot_relative_file("../project/with\\separators", "../bad");

    for path in [&project_dir, &memory_path, &rule_path, &project_path] {
        let rendered = path.to_string_lossy();
        assert!(path.starts_with(memory_archive_dir()));
        assert!(!rendered.contains("../project"));
        assert!(!rendered.contains("../memory"));
        assert!(!rendered.contains("../rules"));
        assert!(!rendered.contains("../bad"));
    }

    assert_eq!(project_dir, memory_archive_dir().join(project_relative_dir));
    assert_eq!(memory_path, memory_archive_dir().join(memory_relative_path));
    assert_eq!(
        project_path,
        memory_archive_dir().join(project_relative_path)
    );
    assert!(rule_path.ends_with("20260516_120000.md"));
    assert!(project_path.ends_with("unknown-timestamp.md"));
}

#[test]
fn cleanup_previous_launch_logs_keeps_current_and_newest_matching_files() {
    let log_dir = tempfile::tempdir().expect("temp log directory");
    for name in [
        "ralphx_2026-07-30_10-00-00.log",
        "ralphx_2026-07-30_11-00-00.log",
        "ralphx_2026-07-30_12-00-00.log",
        "other.log",
        "ralphx_.log",
    ] {
        std::fs::write(log_dir.path().join(name), name).expect("test log file");
    }

    let warnings =
        cleanup_previous_launch_logs(log_dir.path(), "ralphx_2026-07-30_12-00-00.log", 1);

    assert!(
        warnings.is_empty(),
        "unexpected cleanup warnings: {warnings:?}"
    );
    assert!(log_dir
        .path()
        .join("ralphx_2026-07-30_12-00-00.log")
        .exists());
    assert!(log_dir
        .path()
        .join("ralphx_2026-07-30_11-00-00.log")
        .exists());
    assert!(!log_dir
        .path()
        .join("ralphx_2026-07-30_10-00-00.log")
        .exists());
    assert!(log_dir.path().join("other.log").exists());
    assert!(log_dir.path().join("ralphx_.log").exists());
}
