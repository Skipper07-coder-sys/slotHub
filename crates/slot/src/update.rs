#[cfg(any(feature = "device", test))]
use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::mpsc::{channel, Sender};
#[cfg(feature = "device")]
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateJob {
    Check,
    Slot,
    BaseOs,
}

impl UpdateJob {
    pub fn arg(self) -> &'static str {
        match self {
            UpdateJob::Check => "check",
            UpdateJob::Slot => "slot",
            UpdateJob::BaseOs => "baseos",
        }
    }
}

/// What `update.sh check` found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Releases {
    pub slot_installed: String,
    pub slot_latest: String,
    pub baseos_installed: String,
    pub baseos_latest: String,
    pub baseos_url: String,
}

impl Releases {
    pub fn parse(text: &str) -> Releases {
        let mut r = Releases::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().to_string();
            match key.trim() {
                "slot_installed" => r.slot_installed = value,
                "slot_latest" => r.slot_latest = value,
                "baseos_installed" => r.baseos_installed = value,
                "baseos_latest" => r.baseos_latest = value,
                "baseos_url" => r.baseos_url = value,
                _ => {}
            }
        }
        r
    }

    pub fn slot_available(&self) -> Option<&str> {
        (!self.slot_latest.is_empty() && self.slot_latest != self.slot_installed)
            .then_some(self.slot_latest.as_str())
    }

    pub fn baseos_available(&self) -> Option<&str> {
        (!self.baseos_latest.is_empty()
            && !self.baseos_url.is_empty()
            && self.baseos_latest != self.baseos_installed)
            .then_some(self.baseos_latest.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing asked yet.
    Idle,
    Checking,
    Checked(Releases),
    /// GitHub could not be reached.
    Offline,
    Downloading(UpdateJob),
    /// The download is on the card; a restart installs it.
    Ready(UpdateJob),
    /// The download failed, with the script's exit code.
    Failed(UpdateJob, Option<i32>),
}

impl UpdateState {
    pub fn busy(&self) -> bool {
        matches!(self, UpdateState::Checking | UpdateState::Downloading(_))
    }

    pub fn releases(&self) -> Option<&Releases> {
        match self {
            UpdateState::Checked(r) => Some(r),
            _ => None,
        }
    }
}

pub trait UpdateJobs: Send {
    fn ask(&mut self, job: UpdateJob);

    fn state(&self) -> UpdateState;

    /// Called once slot has been running long enough to count as a good
    /// start, ending any trial of a freshly swapped System.
    fn confirm(&mut self);
}

pub fn update_jobs() -> Box<dyn UpdateJobs> {
    Box::new(UpdateQueue)
}

pub struct UpdateQueue;

#[cfg(any(feature = "device", test))]
fn update_command(root: &Path, verb: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/update.sh")).arg(verb);
    cmd
}

#[cfg(feature = "device")]
static STATE: Mutex<UpdateState> = Mutex::new(UpdateState::Idle);

#[cfg(feature = "device")]
fn store(state: UpdateState) {
    *STATE.lock().unwrap_or_else(|e| e.into_inner()) = state;
}

#[cfg(feature = "device")]
fn root() -> std::ffi::OsString {
    std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into())
}

#[cfg(feature = "device")]
fn queue() -> &'static Sender<UpdateJob> {
    static Q: OnceLock<Sender<UpdateJob>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<UpdateJob>();
        std::thread::Builder::new()
            .name("slot-update".into())
            .spawn(move || {
                let root = root();
                for job in rx {
                    let state = match job {
                        UpdateJob::Check => {
                            match update_command(Path::new(&root), "check").output() {
                                Ok(out) if out.status.success() => UpdateState::Checked(
                                    Releases::parse(&String::from_utf8_lossy(&out.stdout)),
                                ),
                                _ => UpdateState::Offline,
                            }
                        }
                        UpdateJob::Slot | UpdateJob::BaseOs => {
                            let exit = update_command(Path::new(&root), job.arg())
                                .status()
                                .ok()
                                .and_then(|s| s.code());
                            match exit {
                                Some(0) => UpdateState::Ready(job),
                                _ => UpdateState::Failed(job, exit),
                            }
                        }
                    };
                    eprintln!("slot: update {}: {state:?}", job.arg());
                    store(state);
                }
            })
            .expect("update thread");
        tx
    })
}

#[cfg(feature = "device")]
impl UpdateJobs for UpdateQueue {
    fn ask(&mut self, job: UpdateJob) {
        store(match job {
            UpdateJob::Check => UpdateState::Checking,
            _ => UpdateState::Downloading(job),
        });
        let _ = queue().send(job);
    }

    fn state(&self) -> UpdateState {
        STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn confirm(&mut self) {
        let root = root();
        std::thread::spawn(move || {
            let _ = update_command(Path::new(&root), "confirm").status();
        });
    }
}

#[cfg(not(feature = "device"))]
impl UpdateJobs for UpdateQueue {
    fn ask(&mut self, _job: UpdateJob) {}

    fn state(&self) -> UpdateState {
        UpdateState::Idle
    }

    fn confirm(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let cmd = update_command(d.path(), "slot");
        let argv: Vec<String> = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let script = d.path().join("System/update.sh");
        assert_eq!(argv, ["/bin/sh", script.to_str().unwrap(), "slot"]);
    }

    #[test]
    fn the_check_lines_say_what_is_new() {
        let r = Releases::parse(
            "slot_installed=v1.7.0\nslot_latest=v1.8.0\nslot_zip=https://x\nbaseos_installed=1.3.0\nbaseos_latest=1.3.0\nbaseos_url=https://y\n",
        );
        assert_eq!(r.slot_available(), Some("v1.8.0"));
        assert_eq!(r.baseos_available(), None);
        let r = Releases::parse("slot_installed=v1.8.0\nslot_latest=v1.8.0\nbaseos_installed=1.3.0\nbaseos_latest=1.4.0\nbaseos_url=\n");
        assert_eq!(r.slot_available(), None, "the installed release is not new");
        assert_eq!(
            r.baseos_available(),
            None,
            "a BaseOS release with no payload for this device is not offered"
        );
        let r = Releases::parse(
            "slot_installed=unknown\nslot_latest=\nbaseos_latest=1.4.0\nbaseos_url=https://y\n",
        );
        assert_eq!(r.slot_available(), None, "no release at all is not new");
        assert_eq!(r.baseos_available(), Some("1.4.0"));
    }
}
