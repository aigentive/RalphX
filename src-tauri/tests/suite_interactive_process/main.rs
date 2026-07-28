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

mod gate1_conversation_identity;
mod gate1_ipr_fast_path_tests;
mod scripted_claude_second_turn;
mod ipr_cleanup_guard_tests;
mod interactive_mode_integration;
mod task_cleanup_service;
mod reconciliation_runner;
mod agentic_client_flows;
mod supervisor_integration;
mod codex_stream_processor;
mod codex_cli_capabilities;
mod execution_types_serde;
mod task_scheduler_service;
