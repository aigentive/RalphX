use std::sync::Arc;

use tracing::{info, warn};

use crate::domain::repositories::ValidationRunRepository;
use crate::AppState;

pub(crate) async fn run_startup_cleanup(app_state: &AppState) {
    // Expire stale pending questions/permissions from previous runs.
    // Must happen before the HTTP server starts accepting agent requests.
    let qs = Arc::clone(&app_state.question_state);
    let ps = Arc::clone(&app_state.permission_state);
    qs.expire_stale_on_startup().await;
    ps.expire_stale_on_startup().await;

    // Periodic sweep for orphaned in-memory pending questions.
    // Cleans up questions from agents that died without resolving them
    // (complement to expire_stale_on_startup which only runs once at boot).
    {
        let qs = Arc::clone(&app_state.question_state);
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                qs.sweep_stale(std::time::Duration::from_secs(900)).await;
            }
        });
    }

    // Validation commands are also spawned under the app process. On restart,
    // any durable running validation row from a previous process is orphaned.
    let validation_run_repo = Arc::clone(&app_state.validation_run_repo);
    mark_orphaned_validation_runs_on_startup(validation_run_repo).await;

    // All spawned processes are Tauri children — app restart means they are dead.
    let process_repo = Arc::clone(&app_state.process_repo);
    match process_repo.fail_all_active("app_restart").await {
        Ok(n) => info!(
            count = n,
            "Marked stale research processes failed on startup"
        ),
        Err(e) => {
            warn!(error = %e, "Failed to mark stale research processes failed on startup")
        }
    }
}

async fn mark_orphaned_validation_runs_on_startup(
    validation_run_repo: Arc<dyn ValidationRunRepository>,
) {
    match validation_run_repo
        .mark_running_runs_error(chrono::Utc::now())
        .await
    {
        Ok(n) if n > 0 => info!(
            count = n,
            "Marked orphaned running validation runs as error on startup"
        ),
        Ok(_) => {}
        Err(e) => warn!(
            error = %e,
            "Failed to mark orphaned running validation runs as error on startup"
        ),
    }
}

#[cfg(test)]
#[path = "startup_cleanup_tests.rs"]
mod startup_cleanup_tests;
