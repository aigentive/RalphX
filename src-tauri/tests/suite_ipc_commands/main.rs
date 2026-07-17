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

#[path = "../common/mod.rs"]
mod common;

mod agent_conversation_start_service;
mod api_key_commands;
mod chat_attachment_commands;
mod conversation_folder_reference_commands;
mod harness_provider_commands;
mod persona_builder_commands;
mod persona_builder_liveness_commands;
mod persona_commands;
mod persona_runtime_attribution;
mod persona_update_approval_commands;
mod project_commands;
mod task_commands;
mod task_step_commands;
mod ui_commands;
mod unified_chat_commands;
