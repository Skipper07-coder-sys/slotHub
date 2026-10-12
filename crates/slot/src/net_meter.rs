use std::path::{Path, PathBuf};

use slot_ui::Millis;

pub const NET_POLL_MS: Millis = 1000;

/// The Wi-Fi radio's traffic and signal, read from the kernel's counters the
/// way muOS and Knulli fill their network status: bytes from
/// /sys/class/net/<iface>/statistics, signal from /proc/net/wireless. The
/// RG34XXSP's RTL8821CS driver has no /proc/net/wireless; its own
/// /proc/net/rtl8821cs/<iface>/rx_signal carries the same RSSI.
pub struct NetMeter {
    stats: PathBuf,
    wireless: PathBuf,
    rx_signal: PathBuf,
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
        .with_rx_signal(Path::new("/proc/net/rtl8821cs/wlan0/rx_signal"))
    }

    pub fn at(stats: &Path, wireless: &Path, iface: &str) -> Self {
        NetMeter {
            stats: stats.to_path_buf(),
            wireless: wireless.to_path_buf(),
            rx_signal: PathBuf::new(),
            ssid_file: PathBuf::new(),
            iface: iface.to_string(),
            last: None,
            rate: None,
            dbm: None,
            ssid: None,
        }
    }

    /// A Realtek driver's rx_signal file, read when /proc/net/wireless has
    /// no level.
    pub fn with_rx_signal(mut self, path: &Path) -> Self {
        self.rx_signal = path.to_path_buf();
        self
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
            .and_then(|t| wireless_dbm(&t, &self.iface))
            .or_else(|| {
                std::fs::read_to_string(&self.rx_signal)
                    .ok()
                    .and_then(|t| rx_signal_dbm(&t))
            });
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

/// The RSSI in dBm from a Realtek rx_signal file ("rssi:-43" and friends);
/// 0 or nothing means no link.
pub fn rx_signal_dbm(text: &str) -> Option<i32> {
    let dbm: i32 = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("rssi:"))?
        .trim()
        .parse()
        .ok()?;
    (dbm < 0).then_some(dbm)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_realtek_rx_signal_file_gives_the_rssi() {
        let file = "rssi:-43\nsignal_strength:57\nsignal_qual:98\n";
        assert_eq!(rx_signal_dbm(file), Some(-43));
        assert_eq!(signal_bars(-43), 4);
        assert_eq!(rx_signal_dbm("rssi:0\n"), None, "no link");
        assert_eq!(rx_signal_dbm(""), None);
    }

    #[test]
    fn with_no_proc_net_wireless_the_bars_come_from_rx_signal() {
        let d = tempfile::tempdir().unwrap();
        let rx = d.path().join("rx_signal");
        std::fs::write(&rx, "rssi:-70\nsignal_strength:30\n").unwrap();
        let mut m = NetMeter::at(d.path(), &d.path().join("wireless"), "wlan0").with_rx_signal(&rx);
        m.sample(0);
        assert_eq!(m.bars(), Some(2));
    }
}
