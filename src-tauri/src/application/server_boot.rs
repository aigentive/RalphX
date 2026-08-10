use std::sync::Arc;

use tauri::Manager;
use tokio::sync::oneshot;

use crate::application::startup_status::{StartupCoordinator, StartupFailureCode};
use crate::application::HttpShutdownHandle;
use crate::commands::ExecutionState;
use crate::http_server;
use crate::{AppError, AppResult, AppState};

pub(crate) async fn recover_agent_workflow_runs_for_startup(
    app_state: Arc<AppState>,
    execution_state: Arc<ExecutionState>,
) -> AppResult<usize> {
    http_server::recover_agent_workflow_runs_for_startup(app_state, execution_state).await
}

pub(crate) async fn start_server_boot(
    http_app_state: Arc<AppState>,
    app_handle: tauri::AppHandle,
    http_execution_state: Arc<ExecutionState>,
    coordinator: Arc<StartupCoordinator>,
    attempt_id: u64,
) -> AppResult<()> {
    coordinator
        .advance(
            attempt_id,
            crate::application::startup_status::StartupStage::BindingLocalRuntime,
        )
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;
    coordinator
        .ensure_current(attempt_id)
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;

    // Build a shutdown trigger and register it as managed state so the
    // Tauri `RunEvent::ExitRequested` handler can fire it at app exit. The
    // server task itself owns a clone wired into axum's graceful shutdown.
    let shutdown = if let Some(existing) = app_handle.try_state::<HttpShutdownHandle>() {
        existing.inner().clone()
    } else {
        let created = HttpShutdownHandle::new();
        if !app_handle.manage(created.clone()) {
            return Err(AppError::Infrastructure(
                "HttpShutdownHandle was already registered during startup".to_string(),
            ));
        }
        created
    };
    coordinator
        .ensure_current(attempt_id)
        .map_err(|error| AppError::Infrastructure(error.to_string()))?;

    // R-8 (PR 1.5-B): publish the very Arcs :3847 is about to serve from, so the remote
    // listener's curated `/api` remount can reuse them instead of building a third `AppState`.
    // Registered here because this is the one seam holding both Arcs and the `AppHandle`.
    // Idempotent, mirroring `HttpShutdownHandle` above: a second boot attempt reuses the
    // existing registration rather than failing the start.
    if app_handle
        .try_state::<Arc<crate::remote_server::fetch_remount::SharedHttpAppState>>()
        .is_none()
    {
        let shared = Arc::new(
            crate::remote_server::fetch_remount::SharedHttpAppState::new(
                Arc::clone(&http_app_state),
                Arc::clone(&http_execution_state),
            ),
        );
        if !app_handle.manage(shared) {
            tracing::warn!(
                "shared :3847 AppState was already registered; keeping the existing registration"
            );
        }
    }

    let (listener_ready_tx, listener_ready_rx) = oneshot::channel();
    let server_shutdown = shutdown.clone();
    let external_mcp_handle = app_handle.clone();
    let external_mcp_supervisor = Arc::new(move || {
        external_mcp_handle
            .try_state::<crate::infrastructure::ExternalMcpHandle>()
            .and_then(|handle| handle.get().cloned())
    });

    // Spawn only one server and wait for its concrete listener-bind result.
    tauri::async_runtime::spawn(async move {
        if let Err(error) = http_server::start_http_server_with_listener_ready(
            http_app_state,
            http_execution_state,
            server_shutdown,
            Some(listener_ready_tx),
            Some(external_mcp_supervisor),
        )
        .await
        {
            tracing::error!(%error, "HTTP server failed");
        }
    });

    settle_listener_bind_handshake(listener_ready_rx, coordinator.as_ref(), attempt_id).await
}

pub(crate) async fn settle_listener_bind_handshake(
    listener_ready_rx: oneshot::Receiver<AppResult<()>>,
    coordinator: &StartupCoordinator,
    attempt_id: u64,
) -> AppResult<()> {
    match listener_ready_rx.await {
        Ok(Ok(())) => coordinator
            .listener_bound(attempt_id)
            .map_err(|error| AppError::Infrastructure(error.to_string())),
        Ok(Err(error)) => {
            coordinator.fail(
                attempt_id,
                StartupFailureCode::LocalRuntimeBind,
                "RalphX could not restore its local services.",
            );
            Err(error)
        }
        Err(_) => {
            coordinator.fail(
                attempt_id,
                StartupFailureCode::LocalRuntimeBind,
                "RalphX could not restore its local services.",
            );
            Err(AppError::Infrastructure(
                "HTTP server ended before reporting listener readiness".to_string(),
            ))
        }
    }
}
