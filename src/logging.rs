//! Per-run file logging bootstrap for `ur` and `ur-tui`.
//!
//! Every process run gets its own log file under `$UR_ROOT/logs/`.
//! The file is always written; `-v` raises the level from `info` to
//! `debug` and optionally mirrors to stderr (CLI only).

use std::path::{Path, PathBuf};
use std::process;

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Holds the log file path and keeps the non-blocking writer alive.
///
/// The `WorkerGuard` flushes buffered log entries when dropped, so
/// this value must be kept alive for the duration of the process.
pub struct LogHandle {
    path: PathBuf,
    _guard: WorkerGuard,
}

impl std::fmt::Debug for LogHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogHandle")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl LogHandle {
    /// Returns the path to this run's log file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Initializes per-run file logging.
///
/// Creates the log directory under `ur_root`, opens a timestamped log
/// file, and installs a global tracing subscriber. When `verbose` is
/// set the file level rises from `info` to `debug`. When
/// `mirror_stderr` is also set, a human-readable copy is written to
/// stderr.
///
/// # Errors
///
/// Returns an error if the log directory or file cannot be created.
pub fn init(
    binary_name: &str,
    ur_root: &Path,
    verbose: bool,
    mirror_stderr: bool,
) -> anyhow::Result<LogHandle> {
    let logs_dir = ur_root.join("logs");
    std::fs::create_dir_all(&logs_dir)
        .with_context(|| format!("cannot create log directory {}", logs_dir.display()))?;

    let timestamp = chrono::Local::now().format("%Y-%m-%dT%H-%M-%S");
    let pid = process::id();
    let filename = format!("{binary_name}-{timestamp}-{pid}.log");
    let log_path = logs_dir.join(&filename);

    let file = std::fs::File::create(&log_path)
        .with_context(|| format!("cannot create log file {}", log_path.display()))?;

    let file_level = if verbose { "ur=debug" } else { "ur=info" };
    let file_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(file_level));

    let (non_blocking, guard) = tracing_appender::non_blocking(file);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(non_blocking)
        .with_filter(file_filter);

    if verbose && mirror_stderr {
        let stderr_filter = EnvFilter::new(file_level);
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_writer(std::io::stderr)
            .with_filter(stderr_filter);

        tracing_subscriber::registry()
            .with(file_layer)
            .with(stderr_layer)
            .init();
    } else {
        tracing_subscriber::registry().with(file_layer).init();
    }

    Ok(LogHandle {
        path: log_path,
        _guard: guard,
    })
}
