use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tracing_subscriber::{fmt, fmt::MakeWriter, prelude::*, Registry};

use super::diagnostic_commands::{
    build_codex_cli_diagnostics_response, log_frontend_error, truncate_frontend_error_field,
    CodexCliProbeStatus, FrontendErrorLogInput,
};

#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for SharedBuf {
    type Writer = SharedBuf;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[test]
fn truncate_frontend_error_field_preserves_unicode_boundaries() {
    assert_eq!(truncate_frontend_error_field("ab🙂cd", 3), "ab🙂");
}

#[test]
fn log_frontend_error_emits_all_fields_through_tracing() {
    let buffer = SharedBuf(Arc::new(Mutex::new(Vec::new())));
    let subscriber =
        Registry::default().with(fmt::layer().with_writer(buffer.clone()).with_ansi(false));

    tracing::subscriber::with_default(subscriber, || {
        log_frontend_error(FrontendErrorLogInput {
            message: "render failed".to_string(),
            component_stack: Some("at Sidebar".to_string()),
            source: Some("ErrorBoundary".to_string()),
        });
    });

    let output = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(output.contains("frontend_error"));
    assert!(output.contains("render failed"));
    assert!(output.contains("at Sidebar"));
    assert!(output.contains("ErrorBoundary"));
}

#[test]
fn build_codex_cli_diagnostics_response_preserves_probe_error_without_capabilities() {
    let response = build_codex_cli_diagnostics_response(
        CodexCliProbeStatus {
            binary_path: Some("/usr/local/bin/codex".to_string()),
            binary_found: true,
            probe_succeeded: false,
            available: false,
            missing_core_exec_features: vec!["exec".to_string()],
            error: Some("Codex CLI is missing required capability: exec".to_string()),
        },
        None,
    );

    assert!(!response.probe_succeeded);
    assert!(!response.has_core_exec_support);
    assert_eq!(
        response.missing_core_exec_features,
        vec!["exec".to_string()]
    );
    assert_eq!(
        response.error.as_deref(),
        Some("Codex CLI is missing required capability: exec")
    );
}
