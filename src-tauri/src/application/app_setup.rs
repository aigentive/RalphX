use std::path::{Path, PathBuf};
use std::sync::Arc;

use ralphx_events::{BusEventSink, EventSink, InternalEventBus, TeeEventSink};

use crate::application::notification_service::WindowFocusState;
use crate::application::runtime_wiring::{
    build_http_app_state, create_main_window, register_managed_state,
};
use crate::application::server_boot::start_server_boot;
use crate::application::setup_settings::initialize_settings_defaults;
use crate::application::startup_cleanup::run_startup_cleanup;
use crate::application::startup_failure_classification::{
    classify_app_state_construction_failure, generic_app_state_construction_failure,
};
use crate::application::startup_pipeline_launch::launch_startup_pipeline_from_handle;
use crate::application::startup_status::{StartupCoordinator, StartupFailureCode, StartupStage};
use crate::application::AppPaths;
use crate::commands::{ActiveProjectState, ExecutionState};
use crate::shell::event_sink::TauriEventSink;
use crate::AppState;
use tauri::Manager;
use tracing::warn;

const BUNDLED_PLUGIN_DIR_REL: &str = "plugins/app";
const BUNDLED_AGENTS_DIR_REL: &str = "agents";
const BUNDLED_CONFIG_DIR_REL: &str = "config";
const GENERATED_RUNTIME_DIR_REL: &str = "generated";
const GENERATED_CLAUDE_PLUGIN_DIR_NAME: &str = "claude-plugin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BundledRuntimePaths {
    pub(super) plugin_dir: PathBuf,
    pub(super) config_dir: Option<PathBuf>,
    pub(super) generated_plugin_dir: PathBuf,
}

type StartupPrFixReviewPublishResumer = Arc<
    dyn crate::application::agent_workspace_pr_supervision_recovery::AgentWorkspacePrFixReviewPublishResumer,
>;
pub(crate) type StartupPrFixReviewPublishResumerFactory = Arc<
    dyn Fn(&AppState, Arc<ExecutionState>) -> Option<StartupPrFixReviewPublishResumer>
        + Send
        + Sync,
>;

pub(crate) struct StartupAttemptLauncher {
    launch: Arc<dyn Fn(u64) + Send + Sync>,
}

impl StartupAttemptLauncher {
    fn new(launch: Arc<dyn Fn(u64) + Send + Sync>) -> Self {
        Self { launch }
    }

    pub(crate) fn launch(&self, attempt_id: u64) {
        (self.launch)(attempt_id);
    }
}

pub(super) fn resolve_bundled_runtime_paths(
    resource_dir: &Path,
    app_data_dir: &Path,
) -> Option<BundledRuntimePaths> {
    let plugin_dir = resource_dir.join(BUNDLED_PLUGIN_DIR_REL);
    let agents_dir = resource_dir.join(BUNDLED_AGENTS_DIR_REL);
    let config_dir = resource_dir.join(BUNDLED_CONFIG_DIR_REL);

    if !plugin_dir.is_dir() || !agents_dir.is_dir() {
        return None;
    }

    Some(BundledRuntimePaths {
        plugin_dir,
        config_dir: config_dir.is_dir().then_some(config_dir),
        generated_plugin_dir: generated_plugin_dir_for_app_data(app_data_dir),
    })
}

pub(super) fn generated_plugin_runtime_profile_component() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub(super) fn generated_plugin_dir_for_app_data(app_data_dir: &Path) -> PathBuf {
    app_data_dir
        .join(GENERATED_RUNTIME_DIR_REL)
        .join(generated_plugin_runtime_profile_component())
        .join(GENERATED_CLAUDE_PLUGIN_DIR_NAME)
}

pub(super) fn configure_bundled_runtime_paths(
    paths: BundledRuntimePaths,
    configure_plugin_dirs: impl FnOnce(PathBuf, PathBuf),
    configure_config_dir: impl FnOnce(PathBuf),
) -> bool {
    configure_plugin_dirs(paths.plugin_dir, paths.generated_plugin_dir);
    if let Some(config_dir) = paths.config_dir {
        configure_config_dir(config_dir);
        true
    } else {
        false
    }
}

fn configure_bundled_runtime_env(app: &tauri::App<tauri::Wry>) {
    let resource_dir = match app.path().resource_dir() {
        Ok(path) => path,
        Err(error) => {
            warn!(%error, "Failed to resolve app resource directory for bundled runtime discovery");
            return;
        }
    };

    let app_data_dir = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(error) => {
            warn!(%error, "Failed to resolve app data directory for bundled runtime discovery");
            return;
        }
    };

    let Some(paths) = resolve_bundled_runtime_paths(&resource_dir, &app_data_dir) else {
        return;
    };

    configure_bundled_runtime_paths(
        paths,
        crate::infrastructure::agents::claude::configure_runtime_plugin_dirs,
        crate::infrastructure::agents::claude::configure_runtime_config_dir,
    );
}

fn spawn_tasks_disabled_startup_reconciliation(
    app_handle: tauri::AppHandle,
    execution_state: Arc<ExecutionState>,
) {
    tauri::async_runtime::spawn(async move {
        let state = app_handle.state::<AppState>();
        let tasks_enabled = state
            .ideation_settings_repo
            .get_settings()
            .await
            .map(|settings| settings.tasks_enabled)
            .map_err(|error| error.to_string());
        match tasks_enabled {
            Ok(true) => {}
            Ok(false) => {
                let service = state
                    .build_tasks_feature_toggle_service(execution_state, Some(app_handle.clone()));
                let failures = service.drain_active_tasks().await;
                if !failures.is_empty() {
                    tracing::error!(
                        task_ids = ?failures,
                        "Tasks OFF startup reconciliation remains incomplete"
                    );
                }
            }
            Err(error) => {
                tracing::error!(
                    error = %error,
                    "Tasks OFF startup reconciliation failed closed while reading settings"
                );
            }
        }
    });
}

pub(crate) fn run_app_setup(
    app: &mut tauri::App<tauri::Wry>,
    init_execution_state: Arc<ExecutionState>,
    startup_execution_state: Arc<ExecutionState>,
    startup_active_project_state: Arc<ActiveProjectState>,
    startup_pr_fix_review_publish_resumer_factory: StartupPrFixReviewPublishResumerFactory,
    http_execution_state: Arc<ExecutionState>,
    startup_coordinator: Arc<StartupCoordinator>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app_handle = app.handle().clone();

    // Remote event capture + the durable sequencer install in the async setup phase below
    // (`install_remote_stream_from_handle`), not here: this point runs before SQLite is open, and
    // "host mode is configured" is a persisted read (§3.4 capture-at-setup, P-23).

    configure_bundled_runtime_env(app);

    // The native window must be visible before SQLite open/migration work begins.
    let window_focus_state = Arc::new(WindowFocusState::default());
    create_main_window(app, Arc::clone(&window_focus_state))?;

    let launcher = Arc::new(StartupAttemptLauncher::new(Arc::new({
        let app_handle = app_handle.clone();
        let window_focus_state = Arc::clone(&window_focus_state);
        let init_execution_state = Arc::clone(&init_execution_state);
        let startup_execution_state = Arc::clone(&startup_execution_state);
        let startup_active_project_state = Arc::clone(&startup_active_project_state);
        let startup_pr_fix_review_publish_resumer_factory =
            Arc::clone(&startup_pr_fix_review_publish_resumer_factory);
        let http_execution_state = Arc::clone(&http_execution_state);
        let startup_coordinator = Arc::clone(&startup_coordinator);
        move |attempt_id| {
            launch_startup_attempt(
                app_handle.clone(),
                Arc::clone(&window_focus_state),
                Arc::clone(&init_execution_state),
                Arc::clone(&startup_execution_state),
                Arc::clone(&startup_active_project_state),
                Arc::clone(&startup_pr_fix_review_publish_resumer_factory),
                Arc::clone(&http_execution_state),
                Arc::clone(&startup_coordinator),
                attempt_id,
            );
        }
    })));
    if !app.manage(Arc::clone(&launcher)) {
        startup_coordinator.fail(
            startup_coordinator.current_attempt_id(),
            StartupFailureCode::AppStateRegistration,
            "RalphX could not initialize its startup controller.",
        );
        return Ok(());
    }

    launcher.launch(startup_coordinator.current_attempt_id());

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn launch_startup_attempt(
    app_handle: tauri::AppHandle,
    window_focus_state: Arc<WindowFocusState>,
    init_execution_state: Arc<ExecutionState>,
    startup_execution_state: Arc<ExecutionState>,
    startup_active_project_state: Arc<ActiveProjectState>,
    startup_pr_fix_review_publish_resumer_factory: StartupPrFixReviewPublishResumerFactory,
    http_execution_state: Arc<ExecutionState>,
    startup_coordinator: Arc<StartupCoordinator>,
    attempt_id: u64,
) {
    tauri::async_runtime::spawn(async move {
        if startup_coordinator
            .advance(attempt_id, StartupStage::OpeningDatabase)
            .is_err()
        {
            return;
        }
        let app_paths = match AppPaths::from_app_handle(&app_handle) {
            Ok(paths) => paths,
            Err(error) => {
                startup_coordinator.fail(
                    attempt_id,
                    StartupFailureCode::AppStateConstruction,
                    "RalphX could not prepare its local workspace.",
                );
                tracing::error!(%error, "Startup paths could not be resolved");
                return;
            }
        };
        // Structural exclusivity: this is the sole point before AppState opens its pooled SQLite
        // connection. Never move VACUUM behind AppState construction or through DbConnection.
        let runtime = crate::infrastructure::agents::claude::database_maintenance_config();
        let compaction_config =
            crate::infrastructure::sqlite::database_maintenance::CompactionConfig {
                auto_enabled: runtime.db_auto_compact_enabled,
                auto_max_db_bytes: runtime.db_auto_compact_max_db_bytes,
                auto_min_freelist_percent: runtime.db_auto_compact_min_freelist_percent,
            };
        match app_paths.database_maintenance_paths().map(|paths| {
            crate::infrastructure::sqlite::database_maintenance::compact_before_pool_opens_at(
                &paths,
                compaction_config,
            )
        }) {
            Ok(Ok(crate::infrastructure::sqlite::database_maintenance::CompactionOutcome::Compacted { reclaimed_bytes })) => {
                tracing::info!(reclaimed_bytes, "Startup database compaction completed before pool open");
            }
            Ok(Ok(crate::infrastructure::sqlite::database_maintenance::CompactionOutcome::Skipped(reason))) => {
                tracing::info!(reason, "Startup database compaction skipped");
            }
            Ok(Ok(crate::infrastructure::sqlite::database_maintenance::CompactionOutcome::NotRequested)) => {}
            Ok(Err(error)) => tracing::error!(%error, "Startup database compaction failed before pool open"),
            Err(error) => tracing::error!(%error, "Startup database compaction could not resolve maintenance paths"),
        }
        if startup_coordinator
            .advance(attempt_id, StartupStage::Migrating)
            .is_err()
        {
            return;
        }

        let internal_event_bus = InternalEventBus::new();
        let events: Arc<dyn EventSink> = Arc::new(TeeEventSink::new(vec![
            Arc::new(TauriEventSink::new(app_handle.clone())) as Arc<dyn EventSink>,
            Arc::new(BusEventSink::new(internal_event_bus.clone())) as Arc<dyn EventSink>,
        ]));
        let construction_handle = app_handle.clone();
        let construction_coordinator = Arc::clone(&startup_coordinator);
        let migration_boot_id = startup_coordinator.snapshot().boot_id;
        let constructed = tokio::task::spawn_blocking(move || {
            AppState::new_production_with_paths_events_and_migration_observer(
                construction_handle,
                app_paths,
                events,
                internal_event_bus,
                move |progress| {
                    if let Err(error) = construction_coordinator.report_progress(
                        attempt_id,
                        progress.completed_units,
                        progress.total_units,
                    ) {
                        tracing::debug!(
                            %error,
                            "Ignoring migration progress from an inactive startup attempt"
                        );
                        return;
                    }
                    tracing::info!(
                        boot_id = migration_boot_id.as_str(),
                        attempt_id,
                        completed_units = progress.completed_units,
                        total_units = progress.total_units,
                        elapsed_ms = progress.elapsed_ms,
                        "Startup migration progress"
                    );
                },
            )
        })
        .await;
        let mut app_state = match constructed {
            Ok(Ok(app_state)) => app_state,
            Ok(Err(error)) => {
                // Disk exhaustion is recoverable by the user, so the failure it
                // reports has to say so instead of the generic sentence below.
                let failure = classify_app_state_construction_failure(&error);
                startup_coordinator.fail(attempt_id, failure.code, failure.diagnostic_summary);
                tracing::error!(%error, "AppState construction failed");
                return;
            }
            Err(error) => {
                let failure = generic_app_state_construction_failure();
                startup_coordinator.fail(attempt_id, failure.code, failure.diagnostic_summary);
                tracing::error!(%error, "AppState construction worker failed");
                return;
            }
        };

        app_state.window_focus_state = window_focus_state;
        app_state.startup_coordinator = Arc::clone(&startup_coordinator);
        crate::commands::workspace_open_commands::warm_workspace_open_target_cache();
        app_state.webhook_publisher = Some(Arc::new(
            crate::infrastructure::ConcreteWebhookPublisher::new(
                Arc::clone(&app_state.webhook_registration_repo),
                Arc::new(crate::infrastructure::HyperWebhookClient::new()),
            ),
        ));

        if startup_coordinator
            .advance(attempt_id, StartupStage::LoadingSettings)
            .is_err()
        {
            return;
        }
        initialize_settings_defaults(&app_state, Arc::clone(&init_execution_state)).await;

        if startup_coordinator
            .advance(attempt_id, StartupStage::StartupCleanup)
            .is_err()
        {
            return;
        }
        run_startup_cleanup(&app_state).await;

        if startup_coordinator
            .advance(attempt_id, StartupStage::RegisteringState)
            .is_err()
        {
            return;
        }
        let http_app_state = match build_http_app_state(&app_state, app_handle.clone()) {
            Ok(state) => state,
            Err(error) => {
                startup_coordinator.fail(
                    attempt_id,
                    StartupFailureCode::AppStateConstruction,
                    "RalphX could not prepare its local services.",
                );
                tracing::error!(%error, "HTTP AppState construction failed");
                return;
            }
        };
        let pr_fix_review_publish_resumer = startup_pr_fix_review_publish_resumer_factory(
            &app_state,
            Arc::clone(&startup_execution_state),
        );
        if let Some(resumer) = pr_fix_review_publish_resumer.as_ref() {
            app_state.install_agent_workspace_pr_fix_review_publish_resumer(Arc::clone(resumer));
        }

        if let Err(error) = register_managed_state(
            &app_handle,
            app_state,
            startup_coordinator.as_ref(),
            attempt_id,
        ) {
            if !startup_coordinator.is_cancelled() {
                startup_coordinator.fail(
                    attempt_id,
                    StartupFailureCode::AppStateRegistration,
                    "RalphX could not register its application state.",
                );
            }
            tracing::error!(%error, "Dynamic AppState registration failed");
            return;
        }

        if startup_coordinator.ensure_current(attempt_id).is_err() {
            return;
        }
        spawn_tasks_disabled_startup_reconciliation(
            app_handle.clone(),
            Arc::clone(&startup_execution_state),
        );
        if let Err(error) = startup_coordinator.install_listeners(attempt_id, || {
            crate::commands::agent_workspace_auto_review::install_agent_workspace_auto_review_listeners(
                app_handle.clone(),
            );
            crate::commands::agent_workspace_auto_publish::install_agent_workspace_auto_publish_listeners(
                app_handle.clone(),
            );
        }) {
            tracing::debug!(%error, "Skipping startup listeners for inactive attempt");
            return;
        }

        if let Err(error) = start_server_boot(
            http_app_state,
            app_handle.clone(),
            http_execution_state,
            Arc::clone(&startup_coordinator),
            attempt_id,
        )
        .await
        {
            if !startup_coordinator.is_cancelled() {
                tracing::error!(%error, "Startup local-runtime bind failed");
            }
            return;
        }
        // A crashed process leaves its `remote_sessions` rows open; nothing else ever closes them,
        // so they must be reconciled before anything can connect in this one.
        crate::remote_server::close_orphaned_remote_sessions_from_handle(&app_handle).await;
        // Capture + sequencer first, and gated only on host mode being CONFIGURED — not on the
        // listener being enabled. That ordering is the point of P-23: events emitted during a
        // disabled-listener window are still recorded, so a reconnect after a re-enable replays
        // them instead of warm-resuming across an unrecorded gap (§5.2, §3.4).
        crate::remote_server::install_remote_stream_from_handle(&app_handle).await;
        // Remote host mode starts in the same setup phase as the local runtime, but only when
        // the persisted `remote_host_settings` row enables it (§5.2). With the flag off or the
        // row absent, nothing listens on the remote port.
        crate::remote_server::auto_start_remote_listener_from_handle(&app_handle).await;
        let state = app_handle.state::<AppState>();
        launch_startup_pipeline_from_handle(
            app_handle.clone(),
            &state,
            startup_execution_state,
            startup_active_project_state,
            pr_fix_review_publish_resumer,
            Arc::clone(&startup_coordinator),
            attempt_id,
        );
    });
}
