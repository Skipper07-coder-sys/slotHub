use std::path::Path;
#[cfg(any(feature = "device", test))]
use std::process::Command;
#[cfg(feature = "device")]
use std::sync::atomic::{AtomicU8, Ordering};
#[cfg(feature = "device")]
use std::sync::mpsc::{channel, Sender};
#[cfg(feature = "device")]
use std::sync::{Mutex, OnceLock};

use slot_ui::WifiNetwork;

pub const WIFI_CONF: &str = "wifi.conf";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiJob {
    On,
    Off,
    Scan,
}

impl WifiJob {
    pub fn arg(self) -> &'static str {
        match self {
            WifiJob::On => "up",
            WifiJob::Off => "down",
            WifiJob::Scan => "scan",
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
            (WifiJob::Scan, _) => WifiState::Off,
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
            WifiJob::Off | WifiJob::Scan => WifiState::Off,
        }
    }
}

pub trait WifiJobs: Send {
    fn ask(&mut self, job: WifiJob);

    fn state(&self) -> WifiState;

    fn scan(&mut self);

    /// What the last scan found, or None while it is still looking.
    fn scanned(&self) -> Option<Vec<WifiNetwork>>;
}

/// One network per line, as `wifi.sh scan` prints them.
/// A network name as wpa_cli prints it, back to what the network sends:
/// wpa_supplicant writes quotes and backslashes as `\"` and `\\`, and any byte
/// outside printable ASCII as `\xNN`, so "Bob’s iPhone" arrives as
/// `Bob\xe2\x80\x99s iPhone`.
pub fn unescape_ssid(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let hex = |at: usize| {
            text.get(at..at + 2)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        };
        match b[i + 1] {
            b'x' => match hex(i + 2) {
                Some(v) => {
                    out.push(v);
                    i += 4;
                    continue;
                }
                None => out.extend_from_slice(b"\\x"),
            },
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'e' => out.push(0x1b),
            c => out.push(c),
        }
        i += 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn parse_scan(text: &str) -> Vec<WifiNetwork> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let signal = parts.next()?.trim().parse().ok()?;
            let secured = parts.next()? == "1";
            let ssid = unescape_ssid(parts.next()?);
            (!ssid.is_empty()).then_some(WifiNetwork {
                ssid,
                secured,
                signal,
            })
        })
        .collect()
}

/// Puts a network at the top of wifi.conf, so it is the one tried first,
/// replacing any older entry for the same name and keeping the rest.
pub fn remember(root: &Path, ssid: &str, psk: &str) -> std::io::Result<()> {
    let path = root.join(WIFI_CONF);
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<(String, Option<String>)> = Vec::new();
    for line in old.lines().map(|l| l.trim_end_matches('\r')) {
        if let Some(name) = line.strip_prefix("ssid=") {
            kept.push((name.to_string(), None));
        } else if let Some(key) = line
            .strip_prefix("psk=")
            .or_else(|| line.strip_prefix("password="))
        {
            if let Some(last) = kept.last_mut() {
                last.1 = Some(key.to_string());
            }
        }
    }
    kept.retain(|(name, _)| name != ssid);
    let mut text = String::from("# Saved by slot. The first network listed is tried first.\n");
    let fresh = (ssid.to_string(), (!psk.is_empty()).then(|| psk.to_string()));
    for (name, key) in std::iter::once(fresh).chain(kept) {
        text.push_str(&format!("\nssid={name}\n"));
        if let Some(key) = key {
            text.push_str(&format!("psk={key}\n"));
        }
    }
    slot_store::atomic_write(&path, text.as_bytes())
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
static SCANNED: Mutex<Option<Vec<WifiNetwork>>> = Mutex::new(None);

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
                    if job == WifiJob::Scan {
                        let found = wifi_command(Path::new(&root), job)
                            .output()
                            .map(|o| parse_scan(&String::from_utf8_lossy(&o.stdout)))
                            .unwrap_or_default();
                        eprintln!("slot: wifi scan found {}", found.len());
                        *SCANNED.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
                        continue;
                    }
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

    fn scan(&mut self) {
        *SCANNED.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let _ = queue().send(WifiJob::Scan);
    }

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        SCANNED.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

#[cfg(not(feature = "device"))]
impl WifiJobs for WifiQueue {
    fn ask(&mut self, _job: WifiJob) {}

    fn state(&self) -> WifiState {
        WifiState::Off
    }

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        Some(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scanned_name_is_read_back_from_wpa_clis_escapes() {
        assert_eq!(
            unescape_ssid("Bob\\xe2\\x80\\x99s iPhone"),
            "Bob\u{2019}s iPhone"
        );
        assert_eq!(unescape_ssid(r#"say \"hi\" \\ ok"#), r#"say "hi" \ ok"#);
        assert_eq!(unescape_ssid("plain"), "plain");
        assert_eq!(unescape_ssid("odd\\xZZ\\"), "odd\\xZZ\\");
    }

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
    fn a_scan_line_is_signal_lock_and_the_whole_name() {
        let nets = parse_scan("-48\t1\tHome\tNet\n-80\t0\tCafe Free\nnoise\n-60\t1\t\n");
        assert_eq!(
            nets,
            [
                WifiNetwork {
                    ssid: "Home\tNet".into(),
                    secured: true,
                    signal: -48
                },
                WifiNetwork {
                    ssid: "Cafe Free".into(),
                    secured: false,
                    signal: -80
                },
            ]
        );
    }

    #[test]
    fn a_network_picked_on_the_handheld_goes_first_and_replaces_its_old_entry() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join(WIFI_CONF),
            "# mine\r\nssid=Home\r\npsk=old password\r\nssid=Cafe\r\n",
        )
        .unwrap();
        remember(d.path(), "Phone", "hotspot 1").unwrap();
        remember(d.path(), "Home", "new password").unwrap();
        let text = std::fs::read_to_string(d.path().join(WIFI_CONF)).unwrap();
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(
            lines,
            [
                "ssid=Home",
                "psk=new password",
                "ssid=Phone",
                "psk=hotspot 1",
                "ssid=Cafe"
            ]
        );
    }

    #[test]
    fn every_state_survives_the_round_trip_through_its_number() {
        for (n, s) in WifiState::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, n);
        }
    }
}
