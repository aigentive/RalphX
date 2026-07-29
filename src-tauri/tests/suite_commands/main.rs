#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

fn tauri_context() -> tauri::Context<tauri::test::MockRuntime> {
    tauri::generate_context!()
}

mod activity_commands;
mod agent_profile_commands;
mod artifact_commands;
mod automation_commands;
mod conversation_stats_commands;
mod execution_commands_running_count;
mod git_commands;
mod methodology_commands;
mod metrics_commands;
mod plan_branch_commands;
mod qa_commands;
mod question_commands;
mod release_notes_commands;
mod research_commands;
mod review_commands;
mod review_service;
mod workflow_commands;
