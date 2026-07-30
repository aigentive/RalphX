use tracing::{info, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Registry};

use crate::utils::redacting_writer::RedactingMakeWriter;
use crate::utils::size_capped_writer::SizeCappedWriter;

pub(crate) fn create_file_log(
    log_dir: &std::path::Path,
    log_filename: &str,
) -> std::io::Result<(std::path::PathBuf, std::fs::File)> {
    std::fs::create_dir_all(log_dir)?;
    // `create_new` refuses to truncate an existing file or follow a pre-existing
    // symlink; timestamped names collide when the app relaunches within one
    // second (for example a crash loop), so retry with a per-attempt suffix.
    let base = log_filename
        .strip_suffix(".log")
        .unwrap_or(log_filename)
        .to_string();
    for attempt in 0..10u32 {
        let candidate = if attempt == 0 {
            format!("{base}.log")
        } else {
            format!("{base}_{attempt}.log")
        };
        let log_path = log_dir.join(&candidate);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log_path)
        {
            Ok(log_file) => return Ok((log_path, log_file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique launch log file after 10 attempts",
    ))
}

pub(crate) fn cleanup_previous_launch_logs_when_enabled(
    file_logging_enabled: bool,
    log_dir: &std::path::Path,
    current_filename: &str,
    keep_previous: usize,
) -> Vec<std::io::Error> {
    if !file_logging_enabled {
        return Vec::new();
    }

    crate::utils::runtime_log_paths::cleanup_previous_launch_logs(
        log_dir,
        current_filename,
        keep_previous,
    )
}

pub(crate) fn initialize_process_bootstrap() -> Option<tracing_appender::non_blocking::WorkerGuard>
{
    if std::env::var_os("RUST_MIN_STACK").is_none() {
        std::env::set_var("RUST_MIN_STACK", "8388608");
    }

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("ralphx=info,warn"));

    let file_logging_enabled = crate::infrastructure::agents::claude::resolve_file_logging_early();
    let (file_logging_max_bytes, file_logging_keep_files) =
        crate::infrastructure::agents::claude::resolve_file_logging_limits_early();

    let (log_guard, file_layer) = if file_logging_enabled {
        let log_dir = crate::utils::runtime_log_paths::app_log_dir();
        let timestamp = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
        let log_filename = format!("ralphx_{timestamp}.log");
        match create_file_log(&log_dir, &log_filename) {
            Ok((log_path, log_file)) => {
                // Cleanup must never target the log we just created, so use the
                // actually-created filename (collision retries may add a suffix).
                let created_filename = log_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| log_filename.clone());
                let cleanup_errors = cleanup_previous_launch_logs_when_enabled(
                    file_logging_enabled,
                    &log_dir,
                    &created_filename,
                    file_logging_keep_files,
                );
                for error in cleanup_errors {
                    eprintln!("Failed to clean up a previous RalphX log file: {error}");
                }

                let (non_blocking_writer, guard) = tracing_appender::non_blocking(
                    SizeCappedWriter::new(log_file, file_logging_max_bytes),
                );
                let layer = fmt::layer()
                    .with_writer(RedactingMakeWriter::new(non_blocking_writer))
                    .with_ansi(false);

                eprintln!("File logging: {}", log_path.display());
                (Some(guard), Some(layer))
            }
            Err(error) => {
                eprintln!(
                    "File logging unavailable; continuing with stderr logging: {}",
                    error
                );
                (None, None)
            }
        }
    } else {
        (None, None)
    };

    let console_layer = fmt::layer().with_writer(RedactingMakeWriter::new(std::io::stderr));

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
