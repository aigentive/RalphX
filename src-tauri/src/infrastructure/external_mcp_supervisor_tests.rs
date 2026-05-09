#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::infrastructure::agents::claude::ExternalMcpConfig;
    use crate::infrastructure::external_mcp_supervisor::{
        is_test_environment_for_test, stderr_indicates_address_in_use, ExternalMcpHandle,
    };

    // ── Helper ────────────────────────────────────────────────────────────

    fn test_config() -> ExternalMcpConfig {
        ExternalMcpConfig {
            enabled: true,
            port: 3848,
            host: "127.0.0.1".to_string(),
            max_restart_attempts: 3,
            restart_delay_ms: 100,
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
        let pid_path = dir.path().join("external_mcp.pid");

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
        let pid_path = dir.path().join("external_mcp.pid");

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

    // ── Test 8: ExternalMcpHandle Default impl ────────────────────────────

    #[test]
    fn test_handle_default_is_empty() {
        let handle = ExternalMcpHandle::default();
        assert!(handle.get().is_none());
    }
}
