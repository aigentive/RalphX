use std::sync::Arc;

use tracing::{info, warn};

use crate::domain::repositories::{RemoteAuditLogRepository, ValidationRunRepository};
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

    // Remote environment registry: resolve staged pending_add/pending_delete rows
    // left behind by crashes (P-27). Spawned because reconciliation may need the
    // network (token re-validation, revoke retries) and must not block startup;
    // the service itself fails closed on unreachable hosts or an unreadable
    // Keychain, so a partial run only defers work to the next boot.
    {
        let remote_environment_service = Arc::clone(&app_state.remote_environment_service);
        tauri::async_runtime::spawn(async move {
            let report = remote_environment_service.reconcile_on_startup().await;
            info!(
                activated = report.activated.len(),
                deleted_husks = report.deleted_husks.len(),
                completed_removals = report.completed_removals.len(),
                needs_repair = report.needs_repair.len(),
                deferred = report.deferred.len(),
                "Remote environment startup reconciliation finished"
            );
        });
    }

    // Remote audit log retention: the :3849 middleware appends a row per auth decision, so
    // without a ceiling the table grows for the life of the install.
    prune_remote_audit_log_on_startup(app_state).await;

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

/// Retention for `remote_audit_log` (§5.5).
///
/// The remote listener writes an audit row per auth decision on the request path, and reads
/// are already capped at 1000 rows, so the only thing that bounds the table is this sweep.
/// Failure is logged, never fatal: a missing sweep costs disk, not correctness.
async fn prune_remote_audit_log_on_startup(app_state: &AppState) {
    const REMOTE_AUDIT_RETENTION_DAYS: i64 = 30;
    let cutoff = crate::remote_server::auth::remote_timestamp(
        chrono::Utc::now() - chrono::Duration::days(REMOTE_AUDIT_RETENTION_DAYS),
    );
    let repo =
        crate::infrastructure::sqlite::SqliteRemoteAccessRepository::from_db(app_state.db.clone());
    match RemoteAuditLogRepository::prune_before(&repo, &cutoff).await {
        Ok(0) => {}
        Ok(pruned) => info!(
            count = pruned,
            days = REMOTE_AUDIT_RETENTION_DAYS,
            "Pruned expired remote audit log rows on startup"
        ),
        Err(error) => warn!(error = %error, "Failed to prune the remote audit log on startup"),
    }
}

#[cfg(test)]
#[path = "startup_cleanup_tests.rs"]
mod startup_cleanup_tests;
