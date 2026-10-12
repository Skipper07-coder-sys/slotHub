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

/// What is installed, read from the card and the system without going online:
/// slot's release tag from System/release (stamped by the release build) and
/// BaseOS's version from /etc/baseos-release. Empty when a file is missing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Installed {
    pub slot: String,
    pub baseos: String,
}

impl Installed {
    pub fn read(root: &Path) -> Installed {
        let baseos_release = std::env::var_os("AGS_BASEOS_RELEASE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "/etc/baseos-release".into());
        Installed {
            slot: std::fs::read_to_string(root.join("System/release"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default(),
            baseos: std::fs::read_to_string(baseos_release)
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find_map(|l| l.strip_prefix("BASEOS_VERSION="))
                        .map(|v| v.trim().trim_matches('"').to_string())
                })
                .unwrap_or_default(),
        }
    }
}

/// How far a download has got, from the lines `update.sh slot|baseos` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Started,
    Percent(u8),
    Verifying,
    Unpacking,
}

impl Step {
    /// Follows one line of the script's output.
    pub fn read(self, line: &str) -> Step {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("progress") => words
                .next()
                .and_then(|w| w.parse::<u8>().ok())
                .map_or(self, |p| Step::Percent(p.min(100))),
            Some("verifying") => Step::Verifying,
            Some("unpacking") => Step::Unpacking,
            _ => self,
        }
    }

    pub fn text(self) -> String {
        match self {
            Step::Started => "Downloading...".into(),
            Step::Percent(p) => format!("Downloading {p}%..."),
            Step::Verifying => "Verifying...".into(),
            Step::Unpacking => "Unpacking...".into(),
        }
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
    Downloading(UpdateJob, Step),
    /// The download is on the card; a restart installs it.
    Ready(UpdateJob),
    /// The download failed, with the script's exit code.
    Failed(UpdateJob, Option<i32>),
}

impl UpdateState {
    pub fn busy(&self) -> bool {
        matches!(self, UpdateState::Checking | UpdateState::Downloading(..))
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

/// Runs a download, following its progress lines into the shared state.
#[cfg(feature = "device")]
fn download(root: &Path, job: UpdateJob) -> Option<i32> {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let mut child = match update_command(root, job.arg())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("slot: update {}: could not start: {e}", job.arg());
            return None;
        }
    };
    if let Some(out) = child.stdout.take() {
        let mut step = Step::Started;
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            step = step.read(&line);
            store(UpdateState::Downloading(job, step));
        }
    }
    child.wait().ok().and_then(|s| s.code())
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
                            let exit = download(Path::new(&root), job);
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
            _ => UpdateState::Downloading(job, Step::Started),
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
    fn what_is_installed_is_read_from_the_card_without_going_online() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(
            Installed::read(d.path()).slot,
            "",
            "no release file is no version"
        );
        std::fs::create_dir_all(d.path().join("System")).unwrap();
        std::fs::write(d.path().join("System/release"), "v1.7.1\n").unwrap();
        assert_eq!(Installed::read(d.path()).slot, "v1.7.1");
    }

    #[test]
    fn a_download_reads_as_percent_then_verifying_then_unpacking() {
        let mut step = Step::Started;
        assert_eq!(step.text(), "Downloading...");
        step = step.read("progress 42");
        assert_eq!(step.text(), "Downloading 42%...");
        step = step.read("some curl noise");
        assert_eq!(step, Step::Percent(42), "an unknown line changed the step");
        step = step.read("progress x");
        assert_eq!(step, Step::Percent(42));
        assert_eq!(step.read("verifying").text(), "Verifying...");
        assert_eq!(step.read("unpacking").text(), "Unpacking...");
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
