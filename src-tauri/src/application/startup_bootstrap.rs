use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, Registry, fmt, prelude::*};

use crate::utils::redacting_writer::RedactingMakeWriter;

pub(crate) fn initialize_process_bootstrap(
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        std::env::set_var("RUST_MIN_STACK", "8388608");
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ralphx=info,warn"));

    let file_logging_enabled = crate::infrastructure::agents::claude::resolve_file_logging_early();

    let (log_guard, file_layer) = if file_logging_enabled {
        let log_dir = crate::utils::runtime_log_paths::app_log_dir();
        std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");

        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let log_filename = format!("ralphx_{timestamp}.log");
        let log_file = std::fs::File::create(log_dir.join(&log_filename))
            .expect("Failed to create log file");

        let (non_blocking_writer, guard) = tracing_appender::non_blocking(log_file);
        let layer = fmt::layer()
            .with_writer(RedactingMakeWriter::new(non_blocking_writer))
            .with_ansi(false);

        eprintln!("File logging: {}", log_dir.join(&log_filename).display());
        (Some(guard), Some(layer))
    } else {
        (None, None)
    };

    let console_layer = fmt::layer().with_writer(RedactingMakeWriter::new(std::io::stdout));

    Registry::default()
        .with(env_filter)
        .with(console_layer)
        .with(file_layer)
        .init();

    let dotenv_paths = [
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.env"),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".env"),
    ];
    for dotenv_path in dotenv_paths {
        match dotenvy::from_path(&dotenv_path) {
            Ok(_) => info!(path = %dotenv_path.display(), "Loaded local environment overrides"),
            Err(dotenvy::Error::Io(err)) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => warn!(
                path = %dotenv_path.display(),
                error = %err,
                "Failed to load local environment overrides"
            ),
        }
    }

    log_guard
}
