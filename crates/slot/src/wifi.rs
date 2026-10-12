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
pub enum WifiJob {
    On,
    Off,
}

impl WifiJob {
    pub fn arg(self) -> &'static str {
        match self {
            WifiJob::On => "up",
            WifiJob::Off => "down",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    Off,
    Connecting,
    Online,
    NoSetup,
    NotFound,
    NoAddress,
    Failed,
}

impl WifiState {
    pub const ALL: [WifiState; 7] = [
        WifiState::Off,
        WifiState::Connecting,
        WifiState::Online,
        WifiState::NoSetup,
        WifiState::NotFound,
        WifiState::NoAddress,
        WifiState::Failed,
    ];

    pub fn after(job: WifiJob, exit: Option<i32>) -> WifiState {
        match (job, exit) {
            (WifiJob::Off, _) => WifiState::Off,
            (_, Some(0)) => WifiState::Online,
            (_, Some(3)) => WifiState::NoSetup,
            (_, Some(4)) => WifiState::NotFound,
            (_, Some(5)) => WifiState::NoAddress,
            _ => WifiState::Failed,
        }
    }

    pub fn during(job: WifiJob) -> WifiState {
        match job {
            WifiJob::On => WifiState::Connecting,
            WifiJob::Off => WifiState::Off,
        }
    }
}

pub trait WifiJobs: Send {
    fn ask(&mut self, job: WifiJob);

    fn state(&self) -> WifiState;
}

pub fn wifi_jobs() -> Box<dyn WifiJobs> {
    Box::new(WifiQueue)
}

pub struct WifiQueue;

#[cfg(any(feature = "device", test))]
fn wifi_command(root: &Path, job: WifiJob) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/wifi.sh")).arg(job.arg());
    cmd
}

#[cfg(feature = "device")]
static STATE: AtomicU8 = AtomicU8::new(WifiState::Off as u8);

#[cfg(feature = "device")]
fn store(state: WifiState) {
    STATE.store(state as u8, Ordering::SeqCst);
}

#[cfg(feature = "device")]
fn load() -> WifiState {
    let n = STATE.load(Ordering::SeqCst);
    WifiState::ALL
        .into_iter()
        .find(|s| *s as u8 == n)
        .unwrap_or(WifiState::Off)
}

#[cfg(feature = "device")]
fn queue() -> &'static Sender<WifiJob> {
    static Q: OnceLock<Sender<WifiJob>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<WifiJob>();
        std::thread::Builder::new()
            .name("slot-wifi".into())
            .spawn(move || {
                let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
                for job in rx {
                    store(WifiState::during(job));
                    let exit = wifi_command(Path::new(&root), job)
                        .status()
                        .ok()
                        .and_then(|s| s.code());
                    eprintln!("slot: wifi {} exited {exit:?}", job.arg());
                    store(WifiState::after(job, exit));
                }
            })
            .expect("wifi thread");
        tx
    })
}

#[cfg(feature = "device")]
impl WifiJobs for WifiQueue {
    fn ask(&mut self, job: WifiJob) {
        store(WifiState::during(job));
        let _ = queue().send(job);
    }

    fn state(&self) -> WifiState {
        load()
    }
}

#[cfg(not(feature = "device"))]
impl WifiJobs for WifiQueue {
    fn ask(&mut self, _job: WifiJob) {}

    fn state(&self) -> WifiState {
        WifiState::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_the_cards_script_through_sh() {
        let d = tempfile::tempdir().unwrap();
        let cmd = wifi_command(d.path(), WifiJob::On);
        let argv: Vec<String> = std::iter::once(cmd.get_program())
            .chain(cmd.get_args())
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let script = d.path().join("System/wifi.sh");
        assert_eq!(argv, ["/bin/sh", script.to_str().unwrap(), "up"]);
    }

    #[test]
    fn the_script_exit_says_how_joining_went() {
        assert_eq!(WifiState::after(WifiJob::On, Some(0)), WifiState::Online);
        assert_eq!(WifiState::after(WifiJob::On, Some(3)), WifiState::NoSetup);
        assert_eq!(WifiState::after(WifiJob::On, Some(4)), WifiState::NotFound);
        assert_eq!(WifiState::after(WifiJob::On, Some(5)), WifiState::NoAddress);
        assert_eq!(WifiState::after(WifiJob::On, Some(1)), WifiState::Failed);
        assert_eq!(WifiState::after(WifiJob::On, None), WifiState::Failed);
        assert_eq!(WifiState::after(WifiJob::Off, Some(1)), WifiState::Off);
    }

    #[test]
    fn every_state_survives_the_round_trip_through_its_number() {
        for (n, s) in WifiState::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, n);
        }
    }
}
