#[cfg(any(feature = "device", test))]
use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(feature = "device")]
use std::sync::mpsc::{channel, Sender};
#[cfg(feature = "device")]
use std::sync::Mutex;
#[cfg(feature = "device")]
use std::sync::OnceLock;
#[cfg(feature = "device")]
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtJob {
    On,
    Off,
    Pair,
}

impl BtJob {
    pub fn arg(self) -> &'static str {
        match self {
            BtJob::On => "on",
            BtJob::Off => "off",
            BtJob::Pair => "pair",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
}

impl BtDevice {
    pub fn parse(status: &str) -> Option<BtDevice> {
        let line = status.lines().next()?.trim();
        let (mac, name) = line.split_once(' ').unwrap_or((line, ""));
        let valid = mac.len() == 17
            && mac.split(':').count() == 6
            && mac
                .split(':')
                .all(|b| b.len() == 2 && b.chars().all(|c| c.is_ascii_hexdigit()));
        valid.then(|| BtDevice {
            mac: mac.to_string(),
            name: name.trim().to_string(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtState {
    Off,
    Starting,
    Ready,
    Searching,
    NoneFound,
    Failed,
}

impl BtState {
    pub fn after(job: BtJob, exit: Option<i32>) -> BtState {
        match (job, exit) {
            (BtJob::Off, _) => BtState::Off,
            (_, Some(0)) => BtState::Ready,
            (BtJob::Pair, Some(3)) => BtState::NoneFound,
            _ => BtState::Failed,
        }
    }

    pub fn during(job: BtJob) -> BtState {
        match job {
            BtJob::On => BtState::Starting,
            BtJob::Off => BtState::Off,
            BtJob::Pair => BtState::Searching,
        }
    }

    pub fn busy(self) -> bool {
        matches!(self, BtState::Starting | BtState::Searching)
    }
}

pub trait BtJobs: Send {
    fn ask(&mut self, job: BtJob);

    fn state(&self) -> BtState;

    fn connected(&mut self) -> bool;

    fn device(&self) -> Option<BtDevice>;
}

#[cfg(feature = "device")]
pub fn bt_jobs() -> Box<dyn BtJobs> {
    Box::new(BtQueue::new())
}

#[cfg(not(feature = "device"))]
pub fn bt_jobs() -> Box<dyn BtJobs> {
    Box::new(BtQueue)
}

#[cfg(not(feature = "device"))]
pub struct BtQueue;

#[cfg(any(feature = "device", test))]
fn bt_command(root: &Path, verb: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/bluetooth.sh")).arg(verb);
    cmd
}

#[cfg(any(feature = "device", test))]
fn has_link(class: &Path) -> bool {
    std::fs::read_dir(class).is_ok_and(|dir| {
        dir.flatten()
            .any(|e| e.file_name().to_string_lossy().contains(':'))
    })
}

#[cfg(feature = "device")]
static STATE: AtomicU8 = AtomicU8::new(BtState::Off as u8);

#[cfg(feature = "device")]
fn store(state: BtState) {
    STATE.store(state as u8, Ordering::SeqCst);
}

#[cfg(feature = "device")]
fn load() -> BtState {
    [
        BtState::Off,
        BtState::Starting,
        BtState::Ready,
        BtState::Searching,
        BtState::NoneFound,
        BtState::Failed,
    ]
    .into_iter()
    .find(|s| *s as u8 == STATE.load(Ordering::SeqCst))
    .unwrap_or(BtState::Off)
}

#[cfg(feature = "device")]
enum Work {
    Job(BtJob),
    Status,
}

#[cfg(feature = "device")]
static DEVICE: Mutex<Option<BtDevice>> = Mutex::new(None);

#[cfg(feature = "device")]
static ASKING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "device")]
const STATUS_EVERY: Duration = Duration::from_secs(2);

#[cfg(feature = "device")]
fn set_device(device: Option<BtDevice>) {
    *DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = device;
}

#[cfg(feature = "device")]
fn root() -> std::ffi::OsString {
    std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into())
}

#[cfg(feature = "device")]
fn queue() -> &'static Sender<Work> {
    static Q: OnceLock<Sender<Work>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<Work>();
        std::thread::Builder::new()
            .name("slot-bluetooth".into())
            .spawn(move || {
                for work in rx {
                    match work {
                        Work::Job(job) => {
                            store(BtState::during(job));
                            let exit = bt_command(Path::new(&root()), job.arg())
                                .status()
                                .ok()
                                .and_then(|s| s.code());
                            eprintln!("slot: bluetooth {} exited {exit:?}", job.arg());
                            if job == BtJob::Off {
                                set_device(None);
                            }
                            store(BtState::after(job, exit));
                        }
                        Work::Status => {
                            let found = bt_command(Path::new(&root()), "status")
                                .output()
                                .ok()
                                .filter(|o| o.status.success())
                                .and_then(|o| BtDevice::parse(&String::from_utf8_lossy(&o.stdout)));
                            if let Some(d) = &found {
                                eprintln!("slot: bluetooth: playing to {} ({})", d.name, d.mac);
                            }
                            set_device(found);
                            ASKING.store(false, Ordering::SeqCst);
                        }
                    }
                }
            })
            .expect("bluetooth thread");
        tx
    })
}

#[cfg(feature = "device")]
pub struct BtQueue {
    asked_at: Option<Instant>,
}

#[cfg(feature = "device")]
impl BtJobs for BtQueue {
    fn ask(&mut self, job: BtJob) {
        store(BtState::during(job));
        let _ = queue().send(Work::Job(job));
    }

    fn state(&self) -> BtState {
        load()
    }

    fn connected(&mut self) -> bool {
        let link = load() != BtState::Off && has_link(Path::new("/sys/class/bluetooth"));
        if !link {
            set_device(None);
        }
        self.poll(link);
        link
    }

    fn device(&self) -> Option<BtDevice> {
        DEVICE.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(feature = "device")]
impl BtQueue {
    fn new() -> Self {
        BtQueue { asked_at: None }
    }
}

// A link comes up before A2DP does, so the status script is asked again every
// couple of seconds until it names headphones BlueALSA can play to.
#[cfg(feature = "device")]
impl BtQueue {
    fn poll(&mut self, linked: bool) {
        if !linked || self.device().is_some() || load().busy() {
            return;
        }
        if self.asked_at.is_some_and(|at| at.elapsed() < STATUS_EVERY) {
            return;
        }
        if !ASKING.swap(true, Ordering::SeqCst) {
            self.asked_at = Some(Instant::now());
            let _ = queue().send(Work::Status);
        }
    }
}

#[cfg(not(feature = "device"))]
impl BtJobs for BtQueue {
    fn ask(&mut self, _job: BtJob) {}

    fn state(&self) -> BtState {
        BtState::Off
    }

    fn connected(&mut self) -> bool {
        false
    }

    fn device(&self) -> Option<BtDevice> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let cmd = bt_command(d.path(), BtJob::Pair.arg());
        let argv: Vec<String> = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let script = d.path().join("System/bluetooth.sh");
        assert_eq!(argv, ["/bin/sh", script.to_str().unwrap(), "pair"]);
    }

    #[test]
    fn the_script_exit_says_how_a_job_went() {
        assert_eq!(BtState::after(BtJob::On, Some(0)), BtState::Ready);
        assert_eq!(BtState::after(BtJob::On, Some(1)), BtState::Failed);
        assert_eq!(BtState::after(BtJob::Pair, Some(0)), BtState::Ready);
        assert_eq!(BtState::after(BtJob::Pair, Some(3)), BtState::NoneFound);
        assert_eq!(BtState::after(BtJob::Pair, None), BtState::Failed);
        assert_eq!(BtState::after(BtJob::Off, Some(1)), BtState::Off);
    }

    #[test]
    fn the_status_line_names_the_headphones() {
        assert_eq!(
            BtDevice::parse("30:7A:D2:85:D8:1D Le battonetes 2\n"),
            Some(BtDevice {
                mac: "30:7A:D2:85:D8:1D".into(),
                name: "Le battonetes 2".into()
            })
        );
        assert_eq!(
            BtDevice::parse("30:7A:D2:85:D8:1D").map(|d| d.name),
            Some(String::new())
        );
        assert_eq!(BtDevice::parse(""), None);
        assert_eq!(BtDevice::parse("not-an-address name"), None);
    }

    #[test]
    fn only_a_connection_handle_counts_as_a_link() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("hci0")).unwrap();
        assert!(!has_link(d.path()));
        std::fs::create_dir(d.path().join("hci0:12")).unwrap();
        assert!(has_link(d.path()));
        assert!(!has_link(&d.path().join("missing")));
    }
}
