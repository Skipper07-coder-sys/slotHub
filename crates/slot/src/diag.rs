//! Diagnostics: the hardware self-test and the bug report, both run by
//! `System/report.sh`, and the crash catcher's note from the run before.

use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::Mutex;

/// Left at the card root by `report.sh crashcheck` when the run before crashed.
pub const CRASH_CAUGHT: &str = "crash/caught";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagJob {
    SelfTest,
    /// The self-test, only when this slot version has not had one yet.
    SelfTestOnce,
    BugReport,
}

impl DiagJob {
    #[cfg(any(feature = "device", test))]
    fn args(self) -> &'static [&'static str] {
        match self {
            DiagJob::SelfTest => &["selftest"],
            DiagJob::SelfTestOnce => &["selftest-once"],
            DiagJob::BugReport => &["bugreport", "asked"],
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DiagState {
    #[default]
    Idle,
    Running(DiagJob),
    /// The self-test ran and found this many problems.
    Tested(u32),
    Saved,
    Failed(DiagJob),
}

impl DiagState {
    /// Follows one line of `report.sh`'s output.
    pub fn read(self, line: &str) -> DiagState {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("problems"), Some(n))
                if !matches!(self, DiagState::Running(DiagJob::BugReport)) =>
            {
                n.parse().map_or(self, DiagState::Tested)
            }
            (Some("saved"), Some(_)) => DiagState::Saved,
            _ => self,
        }
    }

    /// Where a run ends: what it printed, unless the script failed. A
    /// once-only test with nothing to do ends where it started.
    pub fn after(self, job: DiagJob, exit: Option<i32>) -> DiagState {
        match (self, exit) {
            (DiagState::Tested(_), Some(0 | 3)) | (DiagState::Saved, Some(0)) => self,
            (DiagState::Running(DiagJob::SelfTestOnce), Some(0)) => DiagState::Idle,
            _ => DiagState::Failed(job),
        }
    }

    pub fn running(self) -> bool {
        matches!(self, DiagState::Running(_))
    }

    /// What the Self-Test row says.
    pub fn test_text(self) -> String {
        match self {
            DiagState::Running(DiagJob::SelfTest | DiagJob::SelfTestOnce) => "Testing...".into(),
            DiagState::Tested(0) => "All Passed".into(),
            DiagState::Tested(1) => "1 Problem".into(),
            DiagState::Tested(n) => format!("{n} Problems"),
            DiagState::Failed(DiagJob::SelfTest | DiagJob::SelfTestOnce) => "Failed, Retry".into(),
            _ => "Run".into(),
        }
    }

    /// What the Bug Report row says.
    pub fn report_text(self) -> String {
        match self {
            DiagState::Running(DiagJob::BugReport) => "Saving...".into(),
            DiagState::Saved => "Saved in Reports".into(),
            DiagState::Failed(DiagJob::BugReport) => "Failed, Retry".into(),
            _ => "Save".into(),
        }
    }
}

/// Which radios slot means to have up, so the self-test checks only those.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Radios {
    pub wifi: bool,
    pub bluetooth: bool,
}

pub trait DiagJobs: Send {
    fn run(&mut self, job: DiagJob, radios: Radios);

    fn state(&self) -> DiagState;
}

pub fn diag_jobs() -> Box<dyn DiagJobs> {
    Box::new(DiagQueue)
}

pub struct DiagQueue;

#[cfg(any(feature = "device", test))]
fn diag_command(root: &Path, job: DiagJob, radios: Radios) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/report.sh"))
        .args(job.args())
        .env("SLOT_WIFI", if radios.wifi { "1" } else { "0" })
        .env("SLOT_BT", if radios.bluetooth { "1" } else { "0" });
    cmd
}

/// Takes the crash catcher's note, if the run before left one. Seen once: the
/// note goes as it is read.
pub fn take_crash_note(root: &Path) -> bool {
    std::fs::remove_file(root.join(CRASH_CAUGHT)).is_ok()
}

#[cfg(feature = "device")]
static STATE: Mutex<DiagState> = Mutex::new(DiagState::Idle);

#[cfg(feature = "device")]
fn store(state: DiagState) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = state;
}

#[cfg(feature = "device")]
fn load() -> DiagState {
    *STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(feature = "device")]
fn run(root: &Path, job: DiagJob, radios: Radios) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let child = diag_command(root, job, radios)
        .stdout(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            eprintln!("slot: diag: could not start: {e}");
            return store(DiagState::Failed(job));
        }
    };
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            store(load().read(&line));
        }
    }
    let exit = child.wait().ok().and_then(|s| s.code());
    let state = load().after(job, exit);
    eprintln!("slot: diag {job:?} exited {exit:?}: {state:?}");
    store(state);
}

#[cfg(feature = "device")]
impl DiagJobs for DiagQueue {
    fn run(&mut self, job: DiagJob, radios: Radios) {
        if load().running() {
            return;
        }
        store(DiagState::Running(job));
        let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
        let spawned = std::thread::Builder::new()
            .name("slot-diag".into())
            .spawn(move || run(Path::new(&root), job, radios));
        if let Err(e) = spawned {
            eprintln!("slot: diag: thread failed to start: {e}");
            store(DiagState::Failed(job));
        }
    }

    fn state(&self) -> DiagState {
        load()
    }
}

#[cfg(not(feature = "device"))]
impl DiagJobs for DiagQueue {
    fn run(&mut self, _job: DiagJob, _radios: Radios) {}

    fn state(&self) -> DiagState {
        DiagState::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_calls_the_cards_script_with_the_radios_slot_has_on() {
        let d = tempfile::tempdir().unwrap();
        let radios = Radios {
            wifi: true,
            bluetooth: false,
        };
        let cmd = diag_command(d.path(), DiagJob::BugReport, radios);
        assert_eq!(cmd.get_program(), "/bin/sh");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], d.path().join("System/report.sh"));
        assert_eq!(&args[1..], ["bugreport", "asked"]);
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(envs.contains(&("SLOT_WIFI".as_ref(), Some("1".as_ref()))));
        assert!(envs.contains(&("SLOT_BT".as_ref(), Some("0".as_ref()))));
    }

    #[test]
    fn the_self_test_says_how_many_problems_it_found() {
        let s = DiagState::Running(DiagJob::SelfTest);
        assert_eq!(s.test_text(), "Testing...");
        assert_eq!(s.report_text(), "Save");
        let s = s.read("problems 2").after(DiagJob::SelfTest, Some(3));
        assert_eq!(s, DiagState::Tested(2));
        assert_eq!(s.test_text(), "2 Problems");
        assert_eq!(DiagState::Tested(0).test_text(), "All Passed");
        assert_eq!(DiagState::Tested(1).test_text(), "1 Problem");
    }

    #[test]
    fn a_bug_report_keeps_its_own_result_over_the_tests_line() {
        let s = DiagState::Running(DiagJob::BugReport);
        assert_eq!(s.report_text(), "Saving...");
        let s = s
            .read("problems 1")
            .read("saved bugreport-2026-10-12_21-00-00");
        assert_eq!(s.after(DiagJob::BugReport, Some(0)), DiagState::Saved);
        assert_eq!(DiagState::Saved.report_text(), "Saved in Reports");
    }

    #[test]
    fn a_run_that_ends_early_says_it_failed() {
        let s = DiagState::Running(DiagJob::BugReport);
        assert_eq!(
            s.after(DiagJob::BugReport, Some(1)),
            DiagState::Failed(DiagJob::BugReport)
        );
        assert_eq!(
            DiagState::Failed(DiagJob::BugReport).report_text(),
            "Failed, Retry"
        );
        let s = DiagState::Running(DiagJob::SelfTest);
        assert_eq!(
            s.after(DiagJob::SelfTest, None),
            DiagState::Failed(DiagJob::SelfTest)
        );
    }

    #[test]
    fn a_once_only_test_with_nothing_to_do_goes_quiet() {
        let s = DiagState::Running(DiagJob::SelfTestOnce);
        assert_eq!(s.after(DiagJob::SelfTestOnce, Some(0)), DiagState::Idle);
    }

    #[test]
    fn the_crash_note_is_seen_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(!take_crash_note(d.path()));
        std::fs::create_dir_all(d.path().join("crash")).unwrap();
        std::fs::write(d.path().join(CRASH_CAUGHT), "2026-10-12_21-00-00\n").unwrap();
        assert!(take_crash_note(d.path()));
        assert!(!take_crash_note(d.path()));
    }
}
