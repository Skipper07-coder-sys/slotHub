use std::path::{Path, PathBuf};

use slot_ui::Millis;

pub const NET_POLL_MS: Millis = 1000;

/// The Wi-Fi radio's traffic and signal, read from the kernel's counters the
/// way muOS and Knulli fill their network status: bytes from
/// /sys/class/net/<iface>/statistics, signal from /proc/net/wireless.
pub struct NetMeter {
    stats: PathBuf,
    wireless: PathBuf,
    ssid_file: PathBuf,
    iface: String,
    last: Option<(Millis, u64, u64)>,
    rate: Option<(u64, u64)>,
    dbm: Option<i32>,
    ssid: Option<String>,
}

impl NetMeter {
    pub fn system() -> Self {
        NetMeter::at(
            Path::new("/sys/class/net/wlan0/statistics"),
            Path::new("/proc/net/wireless"),
            "wlan0",
        )
        .with_ssid_file(Path::new("/run/slot-wifi.ssid"))
    }

    pub fn at(stats: &Path, wireless: &Path, iface: &str) -> Self {
        NetMeter {
            stats: stats.to_path_buf(),
            wireless: wireless.to_path_buf(),
            ssid_file: PathBuf::new(),
            iface: iface.to_string(),
            last: None,
            rate: None,
            dbm: None,
            ssid: None,
        }
    }

    /// Where wifi.sh leaves the joined network's name.
    pub fn with_ssid_file(mut self, path: &Path) -> Self {
        self.ssid_file = path.to_path_buf();
        self
    }

    /// Reads the counters; the rate is known from the second sample on.
    pub fn sample(&mut self, now: Millis) {
        self.dbm = std::fs::read_to_string(&self.wireless)
            .ok()
            .and_then(|t| wireless_dbm(&t, &self.iface));
        self.ssid = std::fs::read_to_string(&self.ssid_file)
            .ok()
            .map(|t| crate::wifi::unescape_ssid(t.trim()))
            .filter(|t| !t.is_empty());
        let (Some(rx), Some(tx)) = (self.counter("rx_bytes"), self.counter("tx_bytes")) else {
            self.last = None;
            self.rate = None;
            return;
        };
        if let Some((at, last_rx, last_tx)) = self.last {
            let dt = now.saturating_sub(at);
            if dt > 0 {
                let per_s = |now: u64, then: u64| now.saturating_sub(then) * 1000 / dt;
                self.rate = Some((per_s(rx, last_rx), per_s(tx, last_tx)));
            }
        }
        self.last = Some((now, rx, tx));
    }

    pub fn reset(&mut self) {
        self.last = None;
        self.rate = None;
        self.dbm = None;
        self.ssid = None;
    }

    pub fn sampled(&self) -> bool {
        self.last.is_some() || self.dbm.is_some() || self.ssid.is_some()
    }

    /// Bytes a second down and up.
    pub fn rate(&self) -> Option<(u64, u64)> {
        self.rate
    }

    /// The name of the network the radio joined.
    pub fn ssid(&self) -> Option<&str> {
        self.ssid.as_deref()
    }

    pub fn bars(&self) -> Option<u8> {
        self.dbm.map(signal_bars)
    }

    fn counter(&self, name: &str) -> Option<u64> {
        std::fs::read_to_string(self.stats.join(name))
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// The signal level in dBm of `iface` in a /proc/net/wireless table.
pub fn wireless_dbm(table: &str, iface: &str) -> Option<i32> {
    let line = table.lines().find_map(|l| {
        let (name, rest) = l.split_once(':')?;
        (name.trim() == iface).then_some(rest)
    })?;
    // status, link quality, signal level, noise, ...
    let level: f32 = line
        .split_whitespace()
        .nth(2)?
        .trim_end_matches('.')
        .parse()
        .ok()?;
    let dbm = level as i32;
    match dbm {
        0 => None,
        d if d > 0 => Some(d - 256),
        d => Some(d),
    }
}

/// Bars out of four for a signal, on Android's thresholds.
pub fn signal_bars(dbm: i32) -> u8 {
    match dbm {
        d if d >= -55 => 4,
        d if d >= -66 => 3,
        d if d >= -77 => 2,
        _ => 1,
    }
}
