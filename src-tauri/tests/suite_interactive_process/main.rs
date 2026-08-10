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

mod agentic_client_flows;
mod codex_cli_capabilities;
mod codex_stream_processor;
mod execution_types_serde;
mod gate1_conversation_identity;
mod gate1_ipr_fast_path_tests;
mod message_delivery_contract;
mod interactive_mode_integration;
mod ipr_cleanup_guard_tests;
mod reconciliation_runner;
mod scripted_claude_second_turn;
mod supervisor_integration;
mod task_cleanup_service;
mod task_scheduler_service;
