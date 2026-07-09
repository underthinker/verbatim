//! Local, no-network dogfood counters (M4 Phase E): completed dictations and
//! unclean shutdowns, for the crash-free-session-rate acceptance criterion
//! (> 99.5%). The numbers stay in the data dir and are surfaced only via
//! `verbatim stats` for a tester to copy into their report - there is no
//! telemetry and nothing leaves the machine (security posture).
//!
//! Crash detection is a dirty-marker scheme: a run writes `running.lock` at
//! startup and removes it on clean shutdown. If the marker survives to the next
//! startup, the previous run died without shutting down (crash, `kill -9`,
//! power loss) and one crash is counted.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use verbatim_core::event::Event;

const STATS_FILE: &str = "stats.toml";
const RUNNING_MARKER: &str = "running.lock";

/// The persisted counters. Monotonic over the app's lifetime on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counters {
    /// Completed, verified dictations (one per `DictationRecorded`).
    pub sessions: u64,
    /// Runs that ended without a clean shutdown.
    pub crashes: u64,
    /// Unix seconds the counters started accumulating (first run).
    pub since: u64,
}

impl Counters {
    /// Crash-free session rate in `[0.0, 1.0]`. With no sessions yet it is a
    /// vacuous 1.0. Crashes are process-level, so this is a proxy: it treats
    /// each unclean shutdown as one lost session against the completed total.
    pub fn crash_free_session_rate(&self) -> f64 {
        if self.sessions == 0 {
            return 1.0;
        }
        let clean = self.sessions.saturating_sub(self.crashes);
        clean as f64 / self.sessions as f64
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read the counters, defaulting to zero if the file is missing or unreadable
/// (a corrupt counter file must never block the app).
pub fn load(dir: &Path) -> Counters {
    match std::fs::read_to_string(dir.join(STATS_FILE)) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Counters::default(),
    }
}

fn save(dir: &Path, counters: &Counters) {
    let Ok(text) = toml::to_string(counters) else {
        return;
    };
    let _ = std::fs::write(dir.join(STATS_FILE), text);
}

/// Mark a run as started: reconcile a crash from a surviving marker, stamp the
/// first-run time, and write the in-progress marker. Call once at startup.
pub fn begin_run(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let mut counters = load(dir);
    if counters.since == 0 {
        counters.since = now_secs();
    }
    let marker = dir.join(RUNNING_MARKER);
    if marker.exists() {
        // The previous run never removed its marker, so it did not shut down
        // cleanly. Count it once.
        counters.crashes += 1;
    }
    save(dir, &counters);
    let _ = std::fs::write(marker, b"1");
}

/// Mark the current run as cleanly shut down, so it is not counted as a crash
/// on the next startup.
pub fn end_run_clean(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(RUNNING_MARKER));
}

/// Count one completed dictation.
fn record_dictation(dir: &Path) {
    let mut counters = load(dir);
    counters.sessions += 1;
    if counters.since == 0 {
        counters.since = now_secs();
    }
    save(dir, &counters);
}

/// The recorder loop: bump the session counter on every verified delivery.
/// Spawn it on the process's runtime with a fresh subscription.
///
/// ponytail: last-writer-wins on the counter file, single-instance assumption.
/// If two Verbatim processes ever run at once their increments can race and
/// lose a count; the dogfood runs one instance, so a per-file lock is not worth
/// it. Add an advisory lock only if concurrent instances become supported.
pub async fn run_recorder(mut rx: broadcast::Receiver<Event>, dir: PathBuf) {
    loop {
        match rx.recv().await {
            Ok(Event::DictationRecorded { .. }) => record_dictation(&dir),
            Ok(_) => {}
            // A lagged recorder only under-counts; never block the bus for it.
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// A human-readable report block for `verbatim stats`, ready to paste into a
/// dogfood tester report.
pub fn report(dir: &Path) -> String {
    let c = load(dir);
    let rate = c.crash_free_session_rate() * 100.0;
    format!(
        "Verbatim dogfood counters (local, never sent anywhere)\n\
         \n\
         completed dictations : {}\n\
         unclean shutdowns    : {}\n\
         crash-free rate       : {rate:.2}%  (target > 99.5%)\n\
         counting since (unix) : {}\n",
        c.sessions, c.crashes, c.since,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("vbtm-stats-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn counts_sessions_and_persists() {
        let dir = temp_dir();
        begin_run(&dir);
        record_dictation(&dir);
        record_dictation(&dir);
        let c = load(&dir);
        assert_eq!(c.sessions, 2);
        assert_eq!(c.crashes, 0);
        assert!(c.since > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clean_shutdown_is_not_a_crash_but_a_surviving_marker_is() {
        let dir = temp_dir();

        // First run: starts clean, no prior marker -> no crash.
        begin_run(&dir);
        record_dictation(&dir);
        assert_eq!(load(&dir).crashes, 0);
        end_run_clean(&dir);

        // Second run after a clean exit -> still no crash.
        begin_run(&dir);
        assert_eq!(load(&dir).crashes, 0);
        // ...but this one dies without calling end_run_clean (marker survives).

        // Third run finds the surviving marker -> one crash.
        begin_run(&dir);
        assert_eq!(load(&dir).crashes, 1);
        // Sessions are untouched by crash accounting.
        assert_eq!(load(&dir).sessions, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_free_rate_is_bounded_and_vacuous_when_empty() {
        assert_eq!(Counters::default().crash_free_session_rate(), 1.0);

        let mostly_clean = Counters {
            sessions: 1000,
            crashes: 3,
            since: 1,
        };
        assert!((mostly_clean.crash_free_session_rate() - 0.997).abs() < 1e-9);

        // More crashes than sessions saturates at 0, never negative.
        let broken = Counters {
            sessions: 2,
            crashes: 5,
            since: 1,
        };
        assert_eq!(broken.crash_free_session_rate(), 0.0);
    }
}
