#[cfg(any(feature = "device", test))]
use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(feature = "device")]
use std::sync::mpsc::{channel, Sender};
#[cfg(feature = "device")]
use std::sync::OnceLock;

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

    fn connected(&self) -> bool;
}

pub fn bt_jobs() -> Box<dyn BtJobs> {
    Box::new(BtQueue)
}

pub struct BtQueue;

#[cfg(any(feature = "device", test))]
fn bt_command(root: &Path, job: BtJob) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/bluetooth.sh")).arg(job.arg());
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
fn queue() -> &'static Sender<BtJob> {
    static Q: OnceLock<Sender<BtJob>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<BtJob>();
        std::thread::Builder::new()
            .name("slot-bluetooth".into())
            .spawn(move || {
                let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
                for job in rx {
                    store(BtState::during(job));
                    let exit = bt_command(Path::new(&root), job)
                        .status()
                        .ok()
                        .and_then(|s| s.code());
                    eprintln!("slot: bluetooth {} exited {exit:?}", job.arg());
                    store(BtState::after(job, exit));
                }
            })
            .expect("bluetooth thread");
        tx
    })
}

#[cfg(feature = "device")]
impl BtJobs for BtQueue {
    fn ask(&mut self, job: BtJob) {
        store(BtState::during(job));
        let _ = queue().send(job);
    }

    fn state(&self) -> BtState {
        load()
    }

    fn connected(&self) -> bool {
        load() != BtState::Off && has_link(Path::new("/sys/class/bluetooth"))
    }
}

#[cfg(not(feature = "device"))]
impl BtJobs for BtQueue {
    fn ask(&mut self, _job: BtJob) {}

    fn state(&self) -> BtState {
        BtState::Off
    }

    fn connected(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let cmd = bt_command(d.path(), BtJob::Pair);
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
    fn only_a_connection_handle_counts_as_a_link() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("hci0")).unwrap();
        assert!(!has_link(d.path()));
        std::fs::create_dir(d.path().join("hci0:12")).unwrap();
        assert!(has_link(d.path()));
        assert!(!has_link(&d.path().join("missing")));
    }
}
