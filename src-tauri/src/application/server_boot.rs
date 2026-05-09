use std::sync::Arc;

use crate::domain::agents::STANDARD_AGENT_HARNESSES;
use tracing::{info, warn};

use crate::application::harness_runtime_registry::{
    resolve_startup_harness_integration, run_startup_harness_integration,
};
use crate::application::runtime_wiring::build_http_app_state;
use crate::application::TeamStateTracker;
use crate::commands::ExecutionState;
use crate::http_server;
use crate::AppState;

pub(crate) fn start_server_boot(
    app_state: &AppState,
    app_handle: tauri::AppHandle,
    http_execution_state: Arc<ExecutionState>,
    http_team_tracker: TeamStateTracker,
) {
    // Start HTTP server for MCP proxy on the configured local backend port.
    // Create a second AppState sharing the Tauri AppState's DB connection,
    // plus shared in-memory state (question_state, permission_state, message_queue)
    // so MCP handlers and Tauri commands operate on the same data.
    let http_app_state = build_http_app_state(app_state, app_handle)
        .expect("Failed to initialize AppState for HTTP server");
    // Spawn HTTP server with pre-cloned state
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            http_server::start_http_server(http_app_state, http_execution_state, http_team_tracker)
                .await
        {
            tracing::error!("HTTP server failed: {}", e);
        }
    });

    // Run any harness-specific startup integrations, such as Claude MCP registration.
    for harness in STANDARD_AGENT_HARNESSES {
        match resolve_startup_harness_integration(harness) {
            Ok(Some(integration)) => {
                let harness_name = integration.harness();
                let description = integration.description();
                info!("Starting {} {}", harness_name, description);
                tauri::async_runtime::spawn(async move {
                    match run_startup_harness_integration(integration).await {
                        Ok(()) => info!("{} {} succeeded", harness_name, description),
                        Err(e) => warn!("{} {} failed: {}", harness_name, description, e),
                    }
                });
            }
            Ok(None) => {}
            Err(error) => warn!("Skipping {} startup integration: {}", harness, error),
        }
    }
}
