use anyhow::{Context, Result, anyhow};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

/// Initialise application logging.
///
/// Logs are written to stderr for interactive feedback and to a persistent file
/// under the XDG state directory. For this CLI, the best persistent location is:
///
/// ```text
/// $XDG_STATE_HOME/cls/clserver.log
/// ```
///
/// When `XDG_STATE_HOME` is not set, this normally resolves to:
///
/// ```text
/// ~/.local/state/cls/clserver.log
/// ```
///
/// That location is appropriate because command history and operational logs are
/// application state, not user-editable configuration.
pub fn init(program: &str, verbose: bool) -> Result<WorkerGuard> {
    let log_file = crate::paths::log_file(program)?;
    let log_dir = log_file
        .parent()
        .ok_or_else(|| anyhow!("Log file '{}' has no parent directory", log_file.display()))?;
    let log_file_name = log_file
        .file_name()
        .ok_or_else(|| anyhow!("Log file '{}' has no file name", log_file.display()))?;

    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("Failed to create log directory '{}'", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::never(log_dir, log_file_name);
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
    let level = if verbose {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_filter(level);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(false)
        .with_filter(level);

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .try_init()
        .context("Failed to initialise logging")?;

    Ok(guard)
}
