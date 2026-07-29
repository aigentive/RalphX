#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

mod state_machine_flows;
mod qa_system_flows;
mod review_flows;
mod execution_control_flows;
mod per_project_execution_scoping;
mod workflow_integration;
mod artifact_integration;
mod methodology_integration;
mod gsd_integration;
mod research_integration;
mod repository_swapping;
mod linear_webhook_reconciliation;
