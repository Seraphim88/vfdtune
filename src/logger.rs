//! Tiny file logger so librespot's errors are visible. Log: ~/.cache/vfdtune/vfdtune.log
//! (truncated on every start). Warnings/errors are also kept so the UI can show the reason.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::{
    collections::VecDeque,
    fs::File,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

struct Logger {
    file: Mutex<Option<File>>,
    problems: Mutex<VecDeque<String>>,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn log_path() -> PathBuf {
    crate::config::cache_dir().join("vfdtune.log")
}

pub fn init() {
    let _ = std::fs::create_dir_all(crate::config::cache_dir());
    let file = File::create(log_path()).ok();
    let lg = LOGGER.get_or_init(|| Logger {
        file: Mutex::new(file),
        problems: Mutex::new(VecDeque::new()),
    });
    if log::set_logger(lg).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}

/// Most informative recent warning/error (skips librespot's generic "Skipping to next track").
pub fn recent_problem() -> Option<String> {
    let p = LOGGER.get()?.problems.lock().ok()?;
    let pick = p
        .iter()
        .rev()
        .find(|l| !l.contains("Skipping to next track"))
        .or_else(|| p.back())?;
    Some(pick.chars().take(110).collect())
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= Level::Debug
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = writeln!(f, "{} {} {}", r.level(), r.target(), r.args());
            }
        }
        if r.level() <= Level::Warn {
            if let Ok(mut p) = self.problems.lock() {
                if p.len() >= 20 {
                    p.pop_front();
                }
                p.push_back(format!("{}", r.args()));
            }
        }
    }

    fn flush(&self) {}
}
