use tauri::{AppHandle, Runtime, State};

use crate::application::{
    build_agent_issue_report_draft, submit_agent_issue_report as submit_agent_issue_report_service,
    AgentIssueReportDraft, AgentIssueReportEnvironment, AgentIssueReportSubmitResponse, AppState,
    BuildAgentIssueReportInput, SubmitAgentIssueReportInput,
};

#[tauri::command]
pub async fn build_agent_issue_report<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    input: BuildAgentIssueReportInput,
) -> Result<AgentIssueReportDraft, String> {
    let environment = AgentIssueReportEnvironment {
        app_version: app.package_info().version.to_string(),
        os_name: std::env::consts::OS.to_string(),
        os_version: current_os_version(),
        arch: std::env::consts::ARCH.to_string(),
        generated_at: chrono::Utc::now(),
    };
    build_agent_issue_report_draft(&state, input, environment)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn submit_agent_issue_report(
    state: State<'_, AppState>,
    input: SubmitAgentIssueReportInput,
) -> Result<AgentIssueReportSubmitResponse, String> {
    submit_agent_issue_report_service(&state, input)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn current_os_version() -> Option<String> {
    std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|version| version.trim().to_string())
        .filter(|version| !version.is_empty())
}

#[cfg(not(target_os = "macos"))]
fn current_os_version() -> Option<String> {
    None
}
