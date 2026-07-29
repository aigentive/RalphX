use super::diagnostic_commands::{
    build_codex_cli_diagnostics_response, truncate_frontend_error_field, CodexCliProbeStatus,
};

#[test]
fn truncate_frontend_error_field_preserves_unicode_boundaries() {
    assert_eq!(truncate_frontend_error_field("ab🙂cd", 3), "ab🙂");
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
