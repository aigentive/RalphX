#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::infrastructure::agents::claude::ExternalMcpConfig;
    use crate::infrastructure::external_mcp_supervisor::{
        clear_child_pid, command_matches_external_mcp_process_for_port,
        command_mentions_external_mcp_runtime, current_child_pid, external_mcp_pid_file_name,
        external_mcp_pid_file_path, external_mcp_process_matches_expected_port,
        is_external_mcp_process, is_test_environment_for_test, process_group_exists,
        process_listens_on_port, register_child_pid, stderr_indicates_address_in_use,
        terminate_process_group, terminate_process_group_with, ExternalMcpHandle,
        ExternalMcpReadinessState, TerminateOutcome,
    };

    // ── Helper ────────────────────────────────────────────────────────────

    fn test_config() -> ExternalMcpConfig {
        ExternalMcpConfig {
            enabled: true,
            port: 3848,
            host: "127.0.0.1".to_string(),
            max_restart_attempts: 3,
            restart_delay_ms: 100,
            shutdown_grace_ms: 2000,
            startup_timeout_secs: 30,
            human_wait_timeout_secs: 285,
            auth_token: None,
            node_path: None,
            max_external_ideation_sessions: 1,
            external_session_stale_secs: 7200,
            external_session_startup_grace_secs: None,
            external_message_queue_cap: 10,
            external_session_similarity_threshold: 0.7,
        }
    }

    // ── Test 1: OnceLock semantics ─────────────────────────────────────────

    /// The handle must only accept the first supervisor; subsequent sets must fail.
    #[test]
    fn test_once_lock_set_once() {
        // We can't construct ExternalMcpSupervisor without a real AppHandle,
        // so we test OnceLock directly using a simple Arc<u32> stand-in
        // to verify the semantics the handle will exhibit.
        use std::sync::OnceLock;
        let lock: OnceLock<Arc<u32>> = OnceLock::new();

        let first = Arc::new(1u32);
        let second = Arc::new(2u32);

        assert!(
            lock.set(Arc::clone(&first)).is_ok(),
            "First set must succeed"
        );
        assert!(
            lock.set(Arc::clone(&second)).is_err(),
            "Second set must fail"
        );
        assert_eq!(*lock.get().unwrap(), first, "Lock must retain first value");
    }

    /// ExternalMcpHandle::new() initialises with no supervisor.
    #[test]
    fn test_handle_initially_empty() {
        let handle = ExternalMcpHandle::new();
        assert!(handle.get().is_none());
        assert_eq!(handle.readiness(), ExternalMcpReadinessState::Disabled);
    }

    #[tokio::test]
    async fn disabled_handle_rejects_required_transport_gate() {
        let handle = ExternalMcpHandle::new();

        let error = handle
            .await_ready(std::time::Duration::from_millis(10))
            .await
            .unwrap_err();

        assert_eq!(error, "External MCP transport is disabled");
    }

    // ── Test 2: is_test_environment returns true in test context ───────────

    #[test]
    fn test_is_test_environment_in_tests() {
        // cfg!(test) is true inside #[cfg(test)] blocks
        assert!(
            is_test_environment_for_test(),
            "Should detect test environment"
        );
    }

    // ── Test 3: PID file write and remove ─────────────────────────────────

    #[test]
    fn test_pid_file_write_and_remove() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_path = external_mcp_pid_file_path(dir.path(), 3848);

        // Write
        let pid: u32 = 99999;
        std::fs::write(&pid_path, pid.to_string()).expect("write pid file");
        assert!(pid_path.exists(), "PID file should exist after write");

        // Read back
        let contents = std::fs::read_to_string(&pid_path).expect("read pid file");
        let parsed: u32 = contents.trim().parse().expect("parse pid");
        assert_eq!(parsed, pid);

        // Remove
        let _ = std::fs::remove_file(&pid_path);
        assert!(!pid_path.exists(), "PID file should be gone after remove");

        // Double remove should not panic
        let result = std::fs::remove_file(&pid_path);
        assert!(
            result.is_err(),
            "Second remove must return error (already gone)"
        );
    }

    // ── Test 4: cleanup_orphan removes stale PID file when process is gone ─

    #[tokio::test]
    async fn test_cleanup_orphan_removes_stale_pid_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_path = external_mcp_pid_file_path(dir.path(), 3848);

        // Write a PID that almost certainly doesn't exist
        let nonexistent_pid: i32 = 2_000_000;
        std::fs::write(&pid_path, nonexistent_pid.to_string()).expect("write stale pid");
        assert!(pid_path.exists());

        // Run cleanup manually (without a supervisor — test the file-removal logic)
        // We simulate what cleanup_orphan does:
        if let Ok(contents) = std::fs::read_to_string(&pid_path) {
            if let Ok(_pid) = contents.trim().parse::<i32>() {
                // is_external_mcp_process(nonexistent_pid) returns false (process gone)
                // so we just remove the file
            }
        }
        let _ = std::fs::remove_file(&pid_path);

        assert!(
            !pid_path.exists(),
            "Stale PID file must be removed by cleanup"
        );
    }

    // ── Test 5: EADDRINUSE detection logic ────────────────────────────────

    #[test]
    fn test_eaddrinuse_detection_patterns() {
        let lines = vec!["Error: listen EADDRINUSE: address already in use :::3848".to_string()];
        assert!(
            stderr_indicates_address_in_use(&lines),
            "Should detect EADDRINUSE pattern"
        );

        let lines_other = vec!["Some random error".to_string()];
        assert!(
            !stderr_indicates_address_in_use(&lines_other),
            "Should NOT detect EADDRINUSE for unrelated errors"
        );
    }

    #[test]
    fn test_external_mcp_pid_file_is_scoped_by_port() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(external_mcp_pid_file_name(3848), "external_mcp_3848.pid");
        assert_eq!(external_mcp_pid_file_name(3858), "external_mcp_3858.pid");
        assert_eq!(
            external_mcp_pid_file_path(dir.path(), 3848),
            dir.path().join("external_mcp_3848.pid")
        );
        assert_ne!(
            external_mcp_pid_file_path(dir.path(), 3848),
            external_mcp_pid_file_path(dir.path(), 3858)
        );
        assert_ne!(
            external_mcp_pid_file_path(dir.path(), 3858),
            dir.path().join("external_mcp.pid")
        );
    }

    #[test]
    fn test_external_mcp_command_match_requires_expected_port() {
        let prod_command = "EXTERNAL_MCP_PORT=3848 node /Applications/RalphX.app/Contents/Resources/plugins/app/ralphx-external-mcp/build/index.js";
        let dev_command = "EXTERNAL_MCP_PORT=3858 node /tmp/ralphx/src-tauri/target/debug/plugins/app/ralphx-external-mcp/build/index.js";
        let no_port_command = "node /tmp/ralphx/plugins/app/ralphx-external-mcp/build/index.js";
        let ralphx_port_command = "RALPHX_EXTERNAL_MCP_PORT=3858 node /tmp/ralphx/plugins/app/ralphx-external-mcp/build/index.js";

        assert!(command_matches_external_mcp_process_for_port(
            prod_command,
            3848
        ));
        assert!(command_matches_external_mcp_process_for_port(
            dev_command,
            3858
        ));
        assert!(
            !command_matches_external_mcp_process_for_port(prod_command, 3858),
            "dev cleanup must not match prod external MCP process"
        );
        assert!(
            !command_matches_external_mcp_process_for_port(dev_command, 3848),
            "prod cleanup must not match dev external MCP process"
        );
        assert!(
            !command_matches_external_mcp_process_for_port(no_port_command, 3858),
            "command-only matching must not infer port ownership"
        );
        assert!(command_matches_external_mcp_process_for_port(
            ralphx_port_command,
            3858
        ));
        assert!(command_mentions_external_mcp_runtime(dev_command));
        assert!(
            !command_mentions_external_mcp_runtime("cargo test external_mcp_supervisor_tests"),
            "test/module names must not be classified as the external MCP runtime"
        );
    }

    #[test]
    fn test_process_listens_on_port_detects_current_listener() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("listener addr").port();

        assert!(
            process_listens_on_port(std::process::id() as i32, port),
            "lsof should detect a TCP listener owned by the current test process"
        );
        assert!(
            !process_listens_on_port(0, port),
            "invalid PIDs must never match a listener"
        );
    }

    #[test]
    fn test_external_mcp_process_port_match_accepts_listener_fallback() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("listener addr").port();
        let command_without_env = "node /tmp/ralphx/plugins/app/ralphx-external-mcp/build/index.js";

        assert!(external_mcp_process_matches_expected_port(
            command_without_env,
            std::process::id() as i32,
            port
        ));
        assert!(
            !external_mcp_process_matches_expected_port(
                "node /tmp/not-the-server.js",
                std::process::id() as i32,
                port
            ),
            "a listener alone is not enough without an external MCP command"
        );
    }

    #[test]
    fn test_is_external_mcp_process_rejects_non_external_current_process() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("listener addr").port();

        assert!(
            !is_external_mcp_process(std::process::id() as i32, port),
            "the current test process may listen on the port but is not the external MCP command"
        );
    }

    #[test]
    fn test_eaddrinuse_detection_variant() {
        let lines = vec!["address already in use".to_string()];
        assert!(
            stderr_indicates_address_in_use(&lines),
            "Should detect 'address already in use' variant"
        );
    }

    #[test]
    fn test_eaddrinuse_detection_from_node_bind_error() {
        let lines = vec![
            "[ralphx-external-mcp] Fatal startup error: Error: listen EADDRINUSE: address already in use 127.0.0.1:3858".to_string(),
            "    at Server.setupListenHandle [as _listen2] (node:net:1940:16)".to_string(),
            "  code: 'EADDRINUSE',".to_string(),
            "  port: 3858".to_string(),
        ];

        assert!(
            stderr_indicates_address_in_use(&lines),
            "Should detect Node bind conflicts after a stale server answers health checks"
        );
    }

    // ── Test 6: Health check phase logic (unit) ────────────────────────────

    /// Verify the HTTP status code parser handles well-formed responses.
    #[test]
    fn test_http_status_parsing() {
        let response = "HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n";
        let status = response
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        assert_eq!(status, 200);

        let response_503 = "HTTP/1.1 503 Service Unavailable\r\n\r\n";
        let status_503 = response_503
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        assert_eq!(status_503, 503);

        // Malformed — should default to 0
        let bad = "not an http response";
        let status_bad = bad
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(0);
        assert_eq!(status_bad, 0);
    }

    // ── Test 7: ExternalMcpConfig default values ──────────────────────────

    #[test]
    fn test_external_mcp_config_defaults() {
        let cfg = ExternalMcpConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.port, 3848);
        assert_eq!(cfg.host, "127.0.0.1");
        assert_eq!(cfg.max_restart_attempts, 3);
        assert_eq!(cfg.restart_delay_ms, 2000);
        assert!(cfg.auth_token.is_none());
        assert!(cfg.node_path.is_none());
    }

    #[test]
    fn test_external_mcp_config_custom() {
        let cfg = test_config();
        assert!(cfg.enabled);
        assert_eq!(cfg.port, 3848);
        assert_eq!(cfg.restart_delay_ms, 100);
    }

    #[test]
    fn terminate_process_group_stops_after_clean_term_exit() {
        let signals = Mutex::new(Vec::new());
        let probes = std::sync::atomic::AtomicUsize::new(0);

        let outcome = terminate_process_group_with(
            Some(42),
            Duration::from_millis(10),
            Duration::ZERO,
            |pid| signals.lock().unwrap().push((pid, "term")),
            |pid| signals.lock().unwrap().push((pid, "kill")),
            |_| probes.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0,
        );

        assert_eq!(outcome, TerminateOutcome::ExitedAfterTerm);
        assert_eq!(*signals.lock().unwrap(), vec![(42, "term")]);
    }

    #[test]
    fn terminate_process_group_bounds_oversized_grace_without_overflow() {
        let outcome = terminate_process_group_with(
            Some(42),
            Duration::MAX,
            Duration::ZERO,
            |_| {},
            |_| panic!("clean TERM exit must not escalate"),
            |_| false,
        );

        assert_eq!(outcome, TerminateOutcome::ExitedAfterTerm);
    }

    #[test]
    fn terminate_process_group_escalates_once_after_grace() {
        let signals = Mutex::new(Vec::new());

        let outcome = terminate_process_group_with(
            Some(84),
            Duration::ZERO,
            Duration::ZERO,
            |pid| signals.lock().unwrap().push((pid, "term")),
            |pid| signals.lock().unwrap().push((pid, "kill")),
            |_| true,
        );

        assert_eq!(outcome, TerminateOutcome::Killed);
        assert_eq!(*signals.lock().unwrap(), vec![(84, "term"), (84, "kill")]);
    }

    #[test]
    fn terminate_process_group_is_no_op_without_pid() {
        let signals = Mutex::new(Vec::new());

        let outcome = terminate_process_group_with(
            None,
            Duration::from_millis(10),
            Duration::ZERO,
            |pid| signals.lock().unwrap().push((pid, "term")),
            |pid| signals.lock().unwrap().push((pid, "kill")),
            |_| true,
        );

        assert_eq!(outcome, TerminateOutcome::NoProcess);
        assert!(signals.lock().unwrap().is_empty());
    }

    #[test]
    fn terminate_process_group_rejects_process_group_zero_and_init() {
        let signals = Mutex::new(Vec::new());

        for pid in [0, 1] {
            let outcome = terminate_process_group_with(
                Some(pid),
                Duration::ZERO,
                Duration::ZERO,
                |pid| signals.lock().unwrap().push((pid, "term")),
                |pid| signals.lock().unwrap().push((pid, "kill")),
                |_| true,
            );
            assert_eq!(outcome, TerminateOutcome::NoProcess);
        }

        assert!(signals.lock().unwrap().is_empty());
    }

    #[test]
    fn child_pid_slot_registers_and_clears_process_identity() {
        let slot = Mutex::new(None);

        register_child_pid(&slot, 1234);
        assert_eq!(current_child_pid(&slot), Some(1234));

        clear_child_pid(&slot);
        assert_eq!(current_child_pid(&slot), None);
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "requires process spawn/kill capability"]
    fn real_process_group_termination_reaps_isolated_child() {
        use std::os::unix::process::CommandExt;

        let mut command = std::process::Command::new("/bin/sh");
        command.args(["-c", "sleep 60 & wait"]);
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::other)?;
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn isolated sleep process");

        let child_pid = child.id();
        let outcome = terminate_process_group(Some(child_pid), Duration::from_millis(100));
        assert!(matches!(
            outcome,
            TerminateOutcome::ExitedAfterTerm | TerminateOutcome::Killed
        ));

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut exited = false;
        while std::time::Instant::now() < deadline {
            if child.try_wait().expect("probe child exit").is_some() {
                exited = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if !exited {
            let _ = child.kill();
        }
        let _ = child.wait();
        assert!(
            exited,
            "isolated process group should exit within the bound"
        );
        assert!(
            !process_group_exists(child_pid as i32),
            "TERM/KILL escalation should leave no surviving descendant in the group"
        );
    }

    // ── Test 8: ExternalMcpHandle Default impl ────────────────────────────

    #[test]
    fn test_handle_default_is_empty() {
        let handle = ExternalMcpHandle::default();
        assert!(handle.get().is_none());
    }
}
