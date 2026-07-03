#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

#[path = "../support/mod.rs"]
mod support;

mod chat_service_errors;
mod chat_service_context;
mod chat_service_merge;
mod chat_service_pause_flows;
mod chat_session_recovery_integration;
mod pending_session_drain;
mod session_fixes_integration;
mod session_linking_integration;
mod http_helpers;
