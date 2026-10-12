//! Wi-Fi Drive: the card served to a web browser on the home network, so
//! games and labels can be dropped on from a computer or phone. The server is
//! File Browser (filebrowser.org, Apache-2.0), the one muOS ships for the
//! same job; `System/drive.sh` starts and stops it.

use std::collections::BTreeMap;
use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(feature = "device")]
use std::sync::mpsc::{channel, Sender};
#[cfg(feature = "device")]
use std::sync::OnceLock;

use slot_store::Platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveState {
    Off,
    Starting,
    On,
    /// The server is not on the card (a build without it).
    Missing,
    Failed,
}

impl DriveState {
    pub const ALL: [DriveState; 5] = [
        DriveState::Off,
        DriveState::Starting,
        DriveState::On,
        DriveState::Missing,
        DriveState::Failed,
    ];

    pub fn after_start(exit: Option<i32>) -> DriveState {
        match exit {
            Some(0) => DriveState::On,
            Some(3) => DriveState::Missing,
            _ => DriveState::Failed,
        }
    }
}

pub trait DriveJobs: Send {
    fn start(&mut self);

    fn stop(&mut self);

    fn state(&self) -> DriveState;
}

/// What the shelves are built from: every file under Games/ and Labels/ with
/// its size. A change means something was added, removed or is still arriving.
pub fn library_marks(root: &Path) -> BTreeMap<String, u64> {
    let mut marks = BTreeMap::new();
    for top in ["Games", "Labels"] {
        for platform in Platform::ALL {
            let dir = root.join(top).join(platform.dir_name());
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let name = entry.file_name().to_string_lossy().into_owned();
                marks.insert(format!("{top}/{}/{name}", platform.dir_name()), size);
            }
        }
    }
    marks
}

/// The name the device answers to on the network, as BaseOS's mDNS gives it.
pub fn address() -> String {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|h| h.trim().to_string())
        .unwrap_or_default();
    match host.is_empty() {
        true => "rg34xxsp.local".into(),
        false => format!("{host}.local"),
    }
}

pub fn drive_jobs() -> Box<dyn DriveJobs> {
    Box::new(DriveQueue)
}

pub struct DriveQueue;

#[cfg(any(feature = "device", test))]
fn drive_command(root: &Path, verb: &str) -> Command {
    let mut cmd = Command::new("/bin/sh");
    cmd.arg(root.join("System/drive.sh")).arg(verb);
    cmd
}

#[cfg(feature = "device")]
static STATE: AtomicU8 = AtomicU8::new(DriveState::Off as u8);

#[cfg(feature = "device")]
fn store(state: DriveState) {
    STATE.store(state as u8, Ordering::SeqCst);
}

#[cfg(feature = "device")]
fn load() -> DriveState {
    let n = STATE.load(Ordering::SeqCst);
    DriveState::ALL
        .into_iter()
        .find(|s| *s as u8 == n)
        .unwrap_or(DriveState::Off)
}

/// Jobs asked for and not yet finished. A job that a newer one has already
/// replaced leaves the state to it, so turning something off and straight back
/// on never ends reading Off.
#[cfg(feature = "device")]
static PENDING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(feature = "device")]
fn queue() -> &'static Sender<bool> {
    static Q: OnceLock<Sender<bool>> = OnceLock::new();
    Q.get_or_init(|| {
        let (tx, rx) = channel::<bool>();
        std::thread::Builder::new()
            .name("slot-drive".into())
            .spawn(move || {
                let root = std::env::var_os("SLOT_ROOT").unwrap_or_else(|| "/mnt/sdcard".into());
                for on in rx {
                    let verb = if on { "start" } else { "stop" };
                    let exit = drive_command(Path::new(&root), verb)
                        .status()
                        .ok()
                        .and_then(|s| s.code());
                    eprintln!("slot: drive {verb} exited {exit:?}");
                    if PENDING.fetch_sub(1, Ordering::SeqCst) > 1 {
                        continue;
                    }
                    store(match on {
                        true => DriveState::after_start(exit),
                        false => DriveState::Off,
                    });
                }
            })
            .expect("drive thread");
        tx
    })
}

#[cfg(feature = "device")]
impl DriveJobs for DriveQueue {
    fn start(&mut self) {
        store(DriveState::Starting);
        PENDING.fetch_add(1, Ordering::SeqCst);
        let _ = queue().send(true);
    }

    fn stop(&mut self) {
        store(DriveState::Off);
        PENDING.fetch_add(1, Ordering::SeqCst);
        let _ = queue().send(false);
    }

    fn state(&self) -> DriveState {
        load()
    }
}

#[cfg(not(feature = "device"))]
impl DriveJobs for DriveQueue {
    fn start(&mut self) {}

    fn stop(&mut self) {}

    fn state(&self) -> DriveState {
        DriveState::Off
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_server_is_started_through_the_cards_script() {
        let d = tempfile::tempdir().unwrap();
        let cmd = drive_command(d.path(), "start");
        assert_eq!(cmd.get_program(), "/bin/sh");
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(args[0], d.path().join("System/drive.sh"));
        assert_eq!(args[1], "start");
    }

    #[test]
    fn a_new_or_growing_file_changes_the_library_marks() {
        let d = tempfile::tempdir().unwrap();
        let gba = d.path().join("Games/GBA");
        std::fs::create_dir_all(&gba).unwrap();
        let before = library_marks(d.path());
        std::fs::write(gba.join("Emerald.gba"), b"ab").unwrap();
        let added = library_marks(d.path());
        assert_ne!(before, added);
        std::fs::write(gba.join("Emerald.gba"), b"abcd").unwrap();
        assert_ne!(added, library_marks(d.path()));
    }

    #[test]
    fn a_start_that_finds_no_server_says_so() {
        assert_eq!(DriveState::after_start(Some(0)), DriveState::On);
        assert_eq!(DriveState::after_start(Some(3)), DriveState::Missing);
        assert_eq!(DriveState::after_start(Some(1)), DriveState::Failed);
        assert_eq!(DriveState::after_start(None), DriveState::Failed);
    }
}
