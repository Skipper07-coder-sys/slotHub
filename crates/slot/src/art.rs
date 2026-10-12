//! Game artwork: box art for games without a label, fetched over Wi-Fi from
//! the libretro thumbnail server by `System/art.sh`, as RetroArch does.

#[cfg(any(feature = "device", test))]
use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtState {
    Idle,
    /// Fetching the `done`th of `total` games still without art.
    Fetching {
        done: u32,
        total: u32,
    },
    Done {
        found: u32,
        missing: u32,
    },
    Offline,
    Failed,
}

impl ArtState {
    /// Follows one line of `art.sh fetch`'s output.
    pub fn read(self, line: &str) -> ArtState {
        let mut words = line.split_whitespace();
        let verb = words.next();
        let mut num = || words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
        match verb {
            Some("games") => ArtState::Fetching {
                done: 0,
                total: num(),
            },
            Some("progress") => ArtState::Fetching {
                done: num(),
                total: num(),
            },
            Some("done") => ArtState::Done {
                found: num(),
                missing: num(),
            },
            _ => self,
        }
    }

    /// Where a run ends: what it printed last, unless the script failed.
    pub fn after(self, exit: Option<i32>) -> ArtState {
        match (self, exit) {
            (ArtState::Done { .. }, Some(0)) => self,
            (_, Some(4)) => ArtState::Offline,
            _ => ArtState::Failed,
        }
    }

    pub fn running(self) -> bool {
        matches!(self, ArtState::Fetching { .. })
    }

    pub fn text(self) -> Option<String> {
        Some(match self {
            ArtState::Idle => return None,
            ArtState::Fetching { total: 0, .. } => "Looking...".into(),
            ArtState::Fetching { done, total } => format!("Getting {done} of {total}..."),
            ArtState::Done {
                found: 0,
                missing: 0,
            } => "All Found".into(),
            ArtState::Done { found, missing: 0 } => format!("Got {found}"),
            ArtState::Done { found, missing } => format!("Got {found}, {missing} Not Found"),
            ArtState::Offline => "No Connection".into(),
            ArtState::Failed => "Failed, Retry".into(),
        })
    }
}

pub trait ArtJobs: Send {
    fn fetch(&mut self);

    fn state(&self) -> ArtState;
}

pub fn art_jobs() -> Box<dyn ArtJobs> {
    Box::new(ArtQueue)
}

pub struct ArtQueue;

#[cfg(any(feature = "device", test))]
fn art_command(root: &Path) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/art.sh")).arg("fetch");
    cmd
}

#[cfg(feature = "device")]
static STATE: Mutex<ArtState> = Mutex::new(ArtState::Idle);

#[cfg(feature = "device")]
fn store(state: ArtState) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = state;
}

#[cfg(feature = "device")]
fn load() -> ArtState {
    *STATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(feature = "device")]
fn run(root: &Path) {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let child = art_command(root).stdout(Stdio::piped()).spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => {
            eprintln!("slot: art: could not start: {e}");
            return store(ArtState::Failed);
        }
    };
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            store(load().read(&line));
        }
    }
    let exit = child.wait().ok().and_then(|s| s.code());
    let state = load().after(exit);
    eprintln!("slot: art fetch exited {exit:?}: {state:?}");
    store(state);
}

#[cfg(feature = "device")]
impl ArtJobs for ArtQueue {
    fn fetch(&mut self) {
        if load().running() {
            return;
        }
        store(ArtState::Fetching { done: 0, total: 0 });
        let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
        let spawned = std::thread::Builder::new()
            .name("slot-art".into())
            .spawn(move || run(Path::new(&root)));
        if let Err(e) = spawned {
            eprintln!("slot: art: thread failed to start: {e}");
            store(ArtState::Failed);
        }
    }

    fn state(&self) -> ArtState {
        load()
    }
}

#[cfg(not(feature = "device"))]
impl ArtJobs for ArtQueue {
    fn fetch(&mut self) {}

    fn state(&self) -> ArtState {
        ArtState::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fetch_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let cmd = art_command(d.path());
        assert_eq!(cmd.get_program(), "/bin/sh");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], d.path().join("System/art.sh"));
        assert_eq!(args[1], "fetch");
    }

    #[test]
    fn the_scripts_lines_move_the_state_along() {
        let s = ArtState::Fetching { done: 0, total: 0 };
        assert_eq!(s.text().as_deref(), Some("Looking..."));
        let s = s.read("games 3");
        let s = s.read("progress 2 3");
        assert_eq!(s.text().as_deref(), Some("Getting 2 of 3..."));
        let s = s.read("done 2 1").after(Some(0));
        assert_eq!(s.text().as_deref(), Some("Got 2, 1 Not Found"));
        assert_eq!(s.read("noise"), s);
    }

    #[test]
    fn a_run_that_ends_early_says_why() {
        let s = ArtState::Fetching { done: 1, total: 3 };
        assert_eq!(s.after(Some(4)), ArtState::Offline);
        assert_eq!(s.after(Some(0)), ArtState::Failed);
        assert_eq!(s.after(None), ArtState::Failed);
    }
}
