use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::application::agent_workspace_bridge::{
    dispatch_agent_workspace_bridge_events_once_with_deps, AgentWorkspaceBridgeDeps,
};
use crate::application::harness_runtime_registry::resolve_default_external_mcp_bootstrap;
use crate::application::runtime_factory::{build_chat_service_from_deps, ChatRuntimeFactoryDeps};
use crate::commands::ExecutionState;
use crate::domain::repositories::{
    ExternalEventsRepository, MemoryArchiveRepository, MemoryEntryRepository, ProjectRepository,
    TaskRepository,
};
use crate::infrastructure::{ExternalMcpHandle, ExternalMcpSupervisor};
use crate::utils::backend_endpoint::backend_http_port;
use tauri::Manager;
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

const AGENT_WORKSPACE_BRIDGE_DISPATCH_INTERVAL: Duration = Duration::from_secs(5);

pub async fn recover_memory_archive_jobs_on_startup(
    memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    info!("Recovering pending memory archive jobs...");
    let archive_service = Arc::new(crate::application::MemoryArchiveService::new(
        Arc::clone(&memory_archive_repo),
        memory_entry_repo,
        project_repo,
        PathBuf::from("."),
    ));

    let recovered_count = match memory_archive_repo.count_claimable().await {
        Ok(count) => {
            info!(pending_jobs = count, "Found memory archive jobs to recover");
            let mut processed = 0;
            while processed < count {
                match archive_service.process_next_job().await {
                    Ok(true) => processed += 1,
                    Ok(false) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to process archive job during recovery");
                        break;
                    }
                }
            }
            processed
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to count claimable archive jobs");
            0
        }
    };

    if recovered_count > 0 {
        info!(
            recovered = recovered_count,
            "Completed memory archive job recovery"
        );
    }
}

pub fn spawn_watchdog(
    task_scheduler: Arc<dyn crate::domain::state_machine::services::TaskScheduler>,
    task_repo: Arc<dyn TaskRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    tauri::async_runtime::spawn(async move {
        crate::application::ReadyWatchdog::new(task_scheduler, task_repo, project_repo)
            .run_loop()
            .await;
    });
}

pub fn spawn_cleanup_loops(
    external_events_repo: Arc<dyn ExternalEventsRepository>,
    memory_archive_repo: Arc<dyn MemoryArchiveRepository>,
    memory_entry_repo: Arc<dyn MemoryEntryRepository>,
    project_repo: Arc<dyn ProjectRepository>,
) {
    tauri::async_runtime::spawn(async move {
        crate::application::EventCleanupService::new(external_events_repo)
            .run_loop()
            .await;
    });

    tauri::async_runtime::spawn(async move {
        let archive_service = Arc::new(crate::application::MemoryArchiveService::new(
            memory_archive_repo,
            memory_entry_repo,
            project_repo,
            PathBuf::from("."),
        ));

        let mut backoff_duration = Duration::from_secs(0);
        loop {
            if !backoff_duration.is_zero() {
                tracing::debug!(
                    backoff_secs = backoff_duration.as_secs(),
                    "Memory archive job processor backing off after error"
                );
                tokio::time::sleep(backoff_duration).await;
                backoff_duration = Duration::from_secs(0);
            }

            match archive_service.process_next_job().await {
                Ok(true) => {
                    tracing::debug!("Memory archive job processed, checking for more");
                    backoff_duration = Duration::from_secs(0);
                }
                Ok(false) => {
                    tracing::debug!("No memory archive jobs available, sleeping");
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to process memory archive job");
                    backoff_duration = Duration::from_secs(60);
                    tokio::time::sleep(backoff_duration).await;
                }
            }
        }
    });
}

pub(crate) fn spawn_agent_workspace_bridge_dispatcher(
    bridge_deps: AgentWorkspaceBridgeDeps,
    chat_deps: ChatRuntimeFactoryDeps,
    execution_state: Arc<ExecutionState>,
    app_handle: tauri::AppHandle,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(AGENT_WORKSPACE_BRIDGE_DISPATCH_INTERVAL);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            let chat_service = build_chat_service_from_deps(
                Some(app_handle.clone()),
                Some(Arc::clone(&execution_state)),
                &chat_deps,
            );
            match dispatch_agent_workspace_bridge_events_once_with_deps(&bridge_deps, &chat_service)
                .await
            {
                Ok(summary) if summary.wake_up_count > 0 || summary.error_count > 0 => {
                    tracing::info!(
                        projects = summary.project_count,
                        workspaces = summary.workspace_count,
                        wakeups = summary.wake_up_count,
                        queued = summary.queued_wake_up_count,
                        errors = summary.error_count,
                        "Agent workspace bridge dispatcher tick completed"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Agent workspace bridge dispatcher tick failed"
                    );
                }
            }
        }
    });
}

pub async fn maybe_start_external_mcp(
    app_handle: tauri::AppHandle,
    wait_for_backend_ready: impl Fn(
        u16,
        Duration,
    ) -> futures::future::BoxFuture<'static, Result<(), String>>,
) {
    let bootstrap = match resolve_default_external_mcp_bootstrap() {
        Ok(None) => return,
        Ok(Some(bootstrap)) => bootstrap,
        Err(error) => {
            warn!(
                "External MCP bootstrap unavailable, skipping start: {}",
                error
            );
            return;
        }
    };

    let backend_port = backend_http_port();
    match wait_for_backend_ready(backend_port, Duration::from_secs(30)).await {
        Err(e) => {
            warn!("Backend not ready, skipping external MCP start: {}", e);
        }
        Ok(()) => {
            info!(
                port = backend_port,
                "Backend ready, starting external MCP server"
            );
            let app_data_dir = app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let supervisor = Arc::new(ExternalMcpSupervisor::new(
                bootstrap.config,
                app_handle.clone(),
                app_data_dir,
            ));
            match Arc::clone(&supervisor)
                .start(bootstrap.node_path, bootstrap.entry_path)
                .await
            {
                Ok(()) => {
                    let handle = app_handle.state::<ExternalMcpHandle>();
                    if handle.set(supervisor).is_err() {
                        warn!("ExternalMcpHandle already initialized");
                    } else {
                        info!("External MCP supervisor started and registered");
                    }
                }
                Err(e) => {
                    warn!("Failed to start external MCP: {}", e);
                }
            }
        }
    }
}

pub async fn startup_scan_verification_reconciliation(
    svc: Arc<
        crate::application::reconciliation::verification_reconciliation::VerificationReconciliationService,
    >,
    startup_ideation_recovery_claims: &HashSet<String>,
) {
    svc.startup_scan_excluding_external_archive_sessions(startup_ideation_recovery_claims)
        .await;
    tauri::async_runtime::spawn(async move { svc.run_periodic().await });
}

pub fn spawn_recovery_queue_processor(
    recovery_processor: crate::application::reconciliation::recovery_queue::RecoveryQueueProcessor,
) {
    tauri::async_runtime::spawn(async move {
        recovery_processor.run().await;
    });
}
