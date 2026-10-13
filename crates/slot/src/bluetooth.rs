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

use slot_ui::BtFound;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtJob {
    On,
    Off,
    Connect(String),
}

impl BtJob {
    pub fn args(&self) -> Vec<&str> {
        match self {
            BtJob::On => vec!["on"],
            BtJob::Off => vec!["off"],
            BtJob::Connect(mac) => vec!["connect", mac],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
}

fn valid_mac(mac: &str) -> bool {
    mac.len() == 17
        && mac.split(':').count() == 6
        && mac
            .split(':')
            .all(|b| b.len() == 2 && b.chars().all(|c| c.is_ascii_hexdigit()))
}

impl BtDevice {
    pub fn parse(status: &str) -> Option<BtDevice> {
        let line = status.lines().next()?.trim();
        let (mac, name) = line.split_once(' ').unwrap_or((line, ""));
        valid_mac(mac).then(|| BtDevice {
            mac: mac.to_string(),
            name: name.trim().to_string(),
        })
    }
}

/// One pair of headphones per line, as `bluetooth.sh scan` prints them:
/// signal, paired flag, address and the whole name, tab-separated.
pub fn parse_scan(text: &str) -> Vec<BtFound> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let signal = parts.next()?.trim().parse().ok()?;
            let paired = parts.next()? == "1";
            let mac = parts.next()?.trim();
            let name = parts.next().unwrap_or("").trim().to_string();
            valid_mac(mac).then(|| BtFound {
                mac: mac.to_string(),
                name,
                paired,
                signal,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtState {
    Off,
    Starting,
    Ready,
    Connecting,
    Refused,
    Failed,
}

impl BtState {
    pub const ALL: [BtState; 6] = [
        BtState::Off,
        BtState::Starting,
        BtState::Ready,
        BtState::Connecting,
        BtState::Refused,
        BtState::Failed,
    ];

    pub fn after(job: &BtJob, exit: Option<i32>) -> BtState {
        match (job, exit) {
            (BtJob::Off, _) => BtState::Off,
            (_, Some(0)) => BtState::Ready,
            (BtJob::Connect(_), Some(4)) => BtState::Refused,
            _ => BtState::Failed,
        }
    }

    pub fn during(job: &BtJob) -> BtState {
        match job {
            BtJob::On => BtState::Starting,
            BtJob::Off => BtState::Off,
            BtJob::Connect(_) => BtState::Connecting,
        }
    }

    pub fn busy(self) -> bool {
        matches!(self, BtState::Starting | BtState::Connecting)
    }
}

pub trait BtJobs: Send {
    fn ask(&mut self, job: BtJob);

    fn state(&self) -> BtState;

    fn connected(&mut self) -> bool;

    fn device(&self) -> Option<BtDevice>;

    fn scan(&mut self);

    /// What the last scan found, or None while it is still looking.
    fn scanned(&self) -> Option<Vec<BtFound>>;
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
fn bt_command(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/bluetooth.sh")).args(args);
    cmd
}

#[cfg(feature = "device")]
static STATE: AtomicU8 = AtomicU8::new(BtState::Off as u8);

#[cfg(feature = "device")]
fn store(state: BtState) {
    STATE.store(state as u8, Ordering::SeqCst);
}

#[cfg(feature = "device")]
fn load() -> BtState {
    let n = STATE.load(Ordering::SeqCst);
    BtState::ALL
        .into_iter()
        .find(|s| *s as u8 == n)
        .unwrap_or(BtState::Off)
}

#[cfg(feature = "device")]
enum Work {
    Job(BtJob),
    Status,
    Scan,
}

#[cfg(feature = "device")]
static DEVICE: Mutex<Option<BtDevice>> = Mutex::new(None);

#[cfg(feature = "device")]
static SCANNED: Mutex<Option<Vec<BtFound>>> = Mutex::new(None);

#[cfg(feature = "device")]
static ASKING: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "device")]
const STATUS_EVERY: Duration = Duration::from_secs(2);

#[cfg(feature = "device")]
const STATUS_WHILE_CONNECTED: Duration = Duration::from_secs(5);

#[cfg(feature = "device")]
fn set_device(device: Option<BtDevice>) {
    *DEVICE.lock().unwrap_or_else(|e| e.into_inner()) = device;
}

#[cfg(feature = "device")]
fn root() -> std::ffi::OsString {
    std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into())
}

/// Jobs asked for and not yet finished. A job that a newer one has already
/// replaced leaves the state to it, so turning something off and straight back
/// on never ends reading Off.
#[cfg(feature = "device")]
static PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
                            if PENDING.load(Ordering::SeqCst) == 1 {
                                store(BtState::during(&job));
                            }
                            let exit = bt_command(Path::new(&root()), &job.args())
                                .status()
                                .ok()
                                .and_then(|s| s.code());
                            eprintln!("slot: bluetooth {} exited {exit:?}", job.args().join(" "));
                            if job == BtJob::Off {
                                set_device(None);
                            }
                            if PENDING.fetch_sub(1, Ordering::SeqCst) == 1 {
                                store(BtState::after(&job, exit));
                            }
                        }
                        Work::Scan => {
                            let found = bt_command(Path::new(&root()), &["scan"])
                                .output()
                                .map(|o| parse_scan(&String::from_utf8_lossy(&o.stdout)))
                                .unwrap_or_default();
                            eprintln!("slot: bluetooth scan found {}", found.len());
                            *SCANNED.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
                        }
                        Work::Status => {
                            let found = bt_command(Path::new(&root()), &["status"])
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
        store(BtState::during(&job));
        PENDING.fetch_add(1, Ordering::SeqCst);
        let _ = queue().send(Work::Job(job));
    }

    fn state(&self) -> BtState {
        load()
    }

    // The status script decides, not sysfs: on the device the row never said
    // Connected while the headphones were, so no kernel entry is relied on.
    fn connected(&mut self) -> bool {
        if load() == BtState::Off {
            set_device(None);
            return false;
        }
        self.poll();
        self.device().is_some()
    }

    fn device(&self) -> Option<BtDevice> {
        DEVICE.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn scan(&mut self) {
        *SCANNED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = queue().send(Work::Scan);
    }

    fn scanned(&self) -> Option<Vec<BtFound>> {
        SCANNED.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(feature = "device")]
impl BtQueue {
    fn new() -> Self {
        BtQueue { asked_at: None }
    }
}

// The status script is asked every couple of seconds until it names connected
// headphones, then less often to notice them going away.
#[cfg(feature = "device")]
impl BtQueue {
    fn poll(&mut self) {
        if load().busy() {
            return;
        }
        let every = match self.device() {
            Some(_) => STATUS_WHILE_CONNECTED,
            None => STATUS_EVERY,
        };
        if self.asked_at.is_some_and(|at| at.elapsed() < every) {
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

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<BtFound>> {
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let job = BtJob::Connect("AA:BB:CC:00:11:22".into());
        let cmd = bt_command(d.path(), &job.args());
        let argv: Vec<String> = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let script = d.path().join("System/bluetooth.sh");
        assert_eq!(
            argv,
            [
                "/bin/sh",
                script.to_str().unwrap(),
                "connect",
                "AA:BB:CC:00:11:22"
            ]
        );
    }

    #[test]
    fn the_script_exit_says_how_a_job_went() {
        let connect = BtJob::Connect("AA:BB:CC:00:11:22".into());
        assert_eq!(BtState::after(&BtJob::On, Some(0)), BtState::Ready);
        assert_eq!(BtState::after(&BtJob::On, Some(1)), BtState::Failed);
        assert_eq!(BtState::after(&connect, Some(0)), BtState::Ready);
        assert_eq!(BtState::after(&connect, Some(4)), BtState::Refused);
        assert_eq!(BtState::after(&connect, None), BtState::Failed);
        assert_eq!(BtState::after(&BtJob::Off, Some(1)), BtState::Off);
        assert_eq!(BtState::during(&connect), BtState::Connecting);
        for (n, s) in BtState::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, n);
        }
    }

    #[test]
    fn the_status_line_names_the_headphones() {
        assert_eq!(
            BtDevice::parse("AA:BB:CC:00:11:22 My headphones\n"),
            Some(BtDevice {
                mac: "AA:BB:CC:00:11:22".into(),
                name: "My headphones".into()
            })
        );
        assert_eq!(
            BtDevice::parse("AA:BB:CC:00:11:22").map(|d| d.name),
            Some(String::new())
        );
        assert_eq!(BtDevice::parse(""), None);
        assert_eq!(BtDevice::parse("not-an-address name"), None);
    }

    #[test]
    fn a_scan_line_is_signal_paired_address_and_the_whole_name() {
        let found = parse_scan(
            "-40\t1\tAA:BB:CC:00:11:22\tMy Headphones\n-999\t0\tAA:AA:AA:AA:AA:02\t\nnoise\n-50\t0\tnot-an-address\tx\n",
        );
        assert_eq!(
            found,
            [
                BtFound {
                    mac: "AA:BB:CC:00:11:22".into(),
                    name: "My Headphones".into(),
                    paired: true,
                    signal: -40
                },
                BtFound {
                    mac: "AA:AA:AA:AA:AA:02".into(),
                    name: String::new(),
                    paired: false,
                    signal: -999
                },
            ]
        );
    }
}
