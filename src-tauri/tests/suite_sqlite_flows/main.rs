#[test]
fn merged_suite_requires_nextest() {
    if std::env::var_os("NEXTEST").is_none() {
        panic!(
            "merged integration suites must be run with cargo nextest; see .claude/rules/rust-test-execution.md"
        );
    }
}

mod artifact_integration;
mod execution_control_flows;
mod gsd_integration;
mod linear_webhook_reconciliation;
mod methodology_integration;
mod per_project_execution_scoping;
mod qa_system_flows;
mod repository_swapping;
mod research_integration;
mod review_flows;
mod state_machine_flows;
mod workflow_integration;
