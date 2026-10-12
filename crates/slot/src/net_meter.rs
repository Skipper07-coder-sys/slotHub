use std::path::{Path, PathBuf};

use slot_ui::Millis;

pub const NET_POLL_MS: Millis = 1000;

/// The Wi-Fi radio's traffic and signal, read from the kernel's counters the
/// way muOS and Knulli fill their network status: bytes from
/// /sys/class/net/<iface>/statistics, signal from /proc/net/wireless.
pub struct NetMeter {
    stats: PathBuf,
    wireless: PathBuf,
    iface: String,
    last: Option<(Millis, u64, u64)>,
    rate: Option<(u64, u64)>,
    dbm: Option<i32>,
}

impl NetMeter {
    pub fn system() -> Self {
        NetMeter::at(
            Path::new("/sys/class/net/wlan0/statistics"),
            Path::new("/proc/net/wireless"),
            "wlan0",
        )
    }

    pub fn at(stats: &Path, wireless: &Path, iface: &str) -> Self {
        NetMeter {
            stats: stats.to_path_buf(),
            wireless: wireless.to_path_buf(),
            iface: iface.to_string(),
            last: None,
            rate: None,
            dbm: None,
        }
    }

    /// Reads the counters; the rate is known from the second sample on.
    pub fn sample(&mut self, now: Millis) {
        self.dbm = std::fs::read_to_string(&self.wireless)
            .ok()
            .and_then(|t| wireless_dbm(&t, &self.iface));
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
    }

    pub fn sampled(&self) -> bool {
        self.last.is_some() || self.dbm.is_some()
    }

    /// Bytes a second down and up.
    pub fn rate(&self) -> Option<(u64, u64)> {
        self.rate
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
