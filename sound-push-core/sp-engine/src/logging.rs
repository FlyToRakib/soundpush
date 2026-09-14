//! Log files and the runtime log level (plan §28.1).
//!
//! [`init`] installs the process-wide `tracing` subscriber: a non-blocking writer to size-rotated
//! files in the log folder (`soundpush.log`, then `soundpush.1.log`, …; desktop 5 × 10 MB, Android
//! 3 × 2 MB), plus an optional extra layer (Android: logcat). Events are written at `info`, and at
//! `debug` while the "Debug logging" setting is on: the engine applies it with [`set_debug`] and
//! switches it off again after 24 hours ([`crate::settings::DEBUG_LOGGING_SECS`]).
//!
//! Never logged: audio, keys, pairing secrets, full fingerprints. Real-time threads do not log;
//! they count, and the engine reports the counters.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use tracing::Metadata;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::Layer;
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::{Identity, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;

/// Log files are `<FILE_STEM>.log`, `<FILE_STEM>.1.log`, … (newest first).
pub const FILE_STEM: &str = "soundpush";

#[derive(Debug, Clone)]
pub struct LogConfig {
    pub dir: PathBuf,
    /// The current file rotates when it would grow past this.
    pub max_bytes: u64,
    /// Files kept, the current one included.
    pub max_files: usize,
    /// Also record `log` crate records from dependencies. Off when the extra layer itself writes
    /// through `log` (Android logcat), which would loop.
    pub bridge_log_crate: bool,
}

impl LogConfig {
    /// Desktop: 5 × 10 MB.
    pub fn desktop(dir: PathBuf) -> Self {
        Self {
            dir,
            max_bytes: 10 * 1024 * 1024,
            max_files: 5,
            bridge_log_crate: true,
        }
    }

    /// Android: 3 × 2 MB.
    pub fn android(dir: PathBuf) -> Self {
        Self {
            dir,
            max_bytes: 2 * 1024 * 1024,
            max_files: 3,
            bridge_log_crate: false,
        }
    }
}

/// Keeps the background writer running; dropping it flushes the file.
pub struct LogGuard {
    _worker: tracing_appender::non_blocking::WorkerGuard,
}

static DEBUG: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

/// Install file logging (and `extra`, filtered the same way). `None` when the log folder cannot
/// be written or a subscriber is already installed.
pub fn init<L>(config: LogConfig, extra: Option<L>) -> Option<LogGuard>
where
    L: Layer<tracing_subscriber::Registry> + Send + Sync + 'static,
{
    fs::create_dir_all(&config.dir).ok()?;
    remove_daily_files(&config.dir);
    let file = RotatingFile::open(&config.dir, config.max_bytes, config.max_files).ok()?;
    let (writer, worker) = tracing_appender::non_blocking(file);
    let subscriber = tracing_subscriber::registry()
        .with(extra.map(|layer| layer.with_filter(filter_fn(enabled))))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_filter(filter_fn(enabled)),
        );
    let installed = if config.bridge_log_crate {
        subscriber.try_init().is_ok()
    } else {
        tracing::subscriber::set_global_default(subscriber).is_ok()
    };
    if !installed {
        return None;
    }
    INSTALLED.store(true, Ordering::Relaxed);
    Some(LogGuard { _worker: worker })
}

/// File logging only (desktop).
pub fn init_files(config: LogConfig) -> Option<LogGuard> {
    init::<Identity>(config, None)
}

/// True once [`init`] installed the subscriber.
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::Relaxed)
}

/// Write `debug` events too (the "Debug logging" setting).
pub fn set_debug(enabled: bool) {
    if DEBUG.swap(enabled, Ordering::Relaxed) != enabled {
        tracing::info!(debug = enabled, "log level changed");
    }
}

/// The most verbose level currently written.
pub fn max_level() -> LevelFilter {
    if DEBUG.load(Ordering::Relaxed) {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    }
}

fn enabled(meta: &Metadata<'_>) -> bool {
    *meta.level() <= max_level()
}

/// Log files that exist, newest first.
pub fn log_files(dir: &Path, max_files: usize) -> Vec<PathBuf> {
    (0..max_files.max(1))
        .map(|i| file_path(dir, i))
        .filter(|p| p.is_file())
        .collect()
}

fn file_path(dir: &Path, index: usize) -> PathBuf {
    if index == 0 {
        dir.join(format!("{FILE_STEM}.log"))
    } else {
        dir.join(format!("{FILE_STEM}.{index}.log"))
    }
}

/// Earlier versions rotated daily (`soundpush.2026-09-14.log`): those files are removed.
fn remove_daily_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().to_string();
        let date = name
            .strip_prefix(&format!("{FILE_STEM}."))
            .and_then(|n| n.strip_suffix(".log"));
        let daily = date.is_some_and(|d| {
            d.len() == 10
                && d.bytes().enumerate().all(|(i, b)| match i {
                    4 | 7 => b == b'-',
                    _ => b.is_ascii_digit(),
                })
        });
        if daily {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// `soundpush.log`, moved to `soundpush.1.log` (and so on) when it would exceed `max_bytes`.
/// Runs on the non-blocking writer's own thread.
pub struct RotatingFile {
    dir: PathBuf,
    max_bytes: u64,
    max_files: usize,
    file: File,
    written: u64,
}

impl RotatingFile {
    pub fn open(dir: &Path, max_bytes: u64, max_files: usize) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path(dir, 0))?;
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            dir: dir.to_path_buf(),
            max_bytes: max_bytes.max(1024),
            max_files: max_files.max(1),
            file,
            written,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        let _ = fs::remove_file(file_path(&self.dir, self.max_files - 1));
        for i in (1..self.max_files.saturating_sub(1)).rev() {
            let _ = fs::rename(file_path(&self.dir, i), file_path(&self.dir, i + 1));
        }
        if self.max_files > 1 {
            // std opens files with delete sharing on Windows, so the open file can be renamed.
            fs::rename(file_path(&self.dir, 0), file_path(&self.dir, 1))?;
        }
        self.file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(file_path(&self.dir, 0))?;
        self.written = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Could not rotate (e.g. a file is locked): start the current file over rather than grow
        // without bound.
        if self.written > 0
            && self.written + buf.len() as u64 > self.max_bytes
            && self.rotate().is_err()
        {
            self.file.set_len(0)?;
            self.written = 0;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_by_size_and_keeps_a_bounded_number_of_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut file = RotatingFile::open(dir.path(), 1024, 3).unwrap();
        let line = [b'x'; 100];
        for _ in 0..100 {
            file.write_all(&line).unwrap();
        }
        file.flush().unwrap();
        let files = log_files(dir.path(), 3);
        assert_eq!(
            files.len(),
            3,
            "{:?}",
            fs::read_dir(dir.path()).unwrap().count()
        );
        assert_eq!(files[0], dir.path().join("soundpush.log"));
        for path in &files {
            assert!(fs::metadata(path).unwrap().len() <= 1024);
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);

        // Reopening continues the current file.
        let before = fs::metadata(&files[0]).unwrap().len();
        let mut again = RotatingFile::open(dir.path(), 1024, 3).unwrap();
        again.write_all(b"y").unwrap();
        again.flush().unwrap();
        assert_eq!(fs::metadata(&files[0]).unwrap().len(), before + 1);
    }

    #[test]
    fn old_daily_files_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "soundpush.2026-09-14.log",
            "soundpush.log",
            "soundpush.1.log",
            "other.log",
        ] {
            fs::write(dir.path().join(name), "x").unwrap();
        }
        remove_daily_files(dir.path());
        assert!(!dir.path().join("soundpush.2026-09-14.log").exists());
        assert!(dir.path().join("soundpush.log").exists());
        assert!(dir.path().join("soundpush.1.log").exists());
        assert!(dir.path().join("other.log").exists());
    }

    #[test]
    fn debug_switch_raises_the_level() {
        assert_eq!(max_level(), LevelFilter::INFO);
        set_debug(true);
        assert_eq!(max_level(), LevelFilter::DEBUG);
        set_debug(false);
        assert_eq!(max_level(), LevelFilter::INFO);
    }
}
