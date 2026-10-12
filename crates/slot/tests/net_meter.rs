mod common;

use std::fs;
use std::path::Path;

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::net_meter::{signal_bars, wireless_dbm, NetMeter, NET_POLL_MS};
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_input::Action;
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, StatusIcon, WifiNetwork};
use tempfile::TempDir;

const WIRELESS: &str =
    "Inter-| sta-|   Quality        |   Discarded packets               | Missed | WE
 face | tus | link level noise |  nwid  crypt   frag  retry   misc | beacon | 22
 wlan0: 0000   54.  -60.  -256        0      0      0      0      0        0
";

fn counters(dir: &Path, rx: u64, tx: u64) {
    fs::write(dir.join("rx_bytes"), format!("{rx}\n")).unwrap();
    fs::write(dir.join("tx_bytes"), format!("{tx}\n")).unwrap();
}

#[test]
fn the_signal_level_is_read_for_the_named_radio() {
    assert_eq!(wireless_dbm(WIRELESS, "wlan0"), Some(-60));
    assert_eq!(wireless_dbm(WIRELESS, "wlan1"), None);
    let unsigned = WIRELESS.replace("-60.", "196.");
    assert_eq!(wireless_dbm(&unsigned, "wlan0"), Some(-60));
}

#[test]
fn a_stronger_signal_never_shows_fewer_bars() {
    assert_eq!(signal_bars(-40), 4);
    assert_eq!(signal_bars(-60), 3);
    assert_eq!(signal_bars(-70), 2);
    assert_eq!(signal_bars(-90), 1);
    let mut last = 0;
    for dbm in -100..-30 {
        assert!(signal_bars(dbm) >= last);
        last = signal_bars(dbm);
    }
}

#[test]
fn the_rate_is_the_bytes_moved_since_the_last_sample() {
    let d = TempDir::new().unwrap();
    let wireless = d.path().join("wireless");
    fs::write(&wireless, WIRELESS).unwrap();
    let mut net = NetMeter::at(d.path(), &wireless, "wlan0");
    counters(d.path(), 1_000, 500);
    net.sample(10_000);
    assert_eq!(net.rate(), None, "one sample gives no rate");
    assert_eq!(net.bars(), Some(3));
    counters(d.path(), 51_000, 2_500);
    net.sample(12_000);
    assert_eq!(net.rate(), Some((25_000, 1_000)));
    counters(d.path(), 10, 10);
    net.sample(13_000);
    assert_eq!(
        net.rate(),
        Some((0, 0)),
        "a counter reset reads as no traffic"
    );
}

#[test]
fn no_counters_means_no_rate() {
    let d = TempDir::new().unwrap();
    let mut net = NetMeter::at(&d.path().join("gone"), &d.path().join("gone"), "wlan0");
    net.sample(0);
    net.sample(1_000);
    assert_eq!(net.rate(), None);
    assert_eq!(net.bars(), None);
}

struct Wifi(WifiState);

impl WifiJobs for Wifi {
    fn ask(&mut self, _job: WifiJob) {}

    fn state(&self) -> WifiState {
        self.0
    }

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        Some(Vec::new())
    }
}

fn booted(wifi_on: bool, state: WifiState) -> (TempDir, App) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            wifi: wifi_on,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    a.set_wifi_jobs(Box::new(Wifi(state)));
    (d, a)
}

#[test]
fn the_wifi_icon_follows_the_radio() {
    for (on, state, icon, lit) in [
        (false, WifiState::Online, StatusIcon::WifiOff, false),
        (true, WifiState::Off, StatusIcon::WifiOff, false),
        (
            true,
            WifiState::Connecting,
            StatusIcon::WifiSearching,
            false,
        ),
        (true, WifiState::NotFound, StatusIcon::WifiTrouble, false),
        (true, WifiState::Online, StatusIcon::Wifi4, true),
    ] {
        let (_d, a) = booted(on, state);
        assert_eq!(a.status_icons()[1], (icon, lit), "{on} {state:?}");
    }
}

#[test]
fn bluetooth_off_shows_the_crossed_out_icon() {
    let (_d, a) = booted(false, WifiState::Off);
    assert_eq!(a.status_icons()[0], (StatusIcon::BtOff, false));
}

#[test]
fn the_meter_runs_only_in_the_menu_on_a_connected_wifi() {
    let (d, mut a) = booted(true, WifiState::Online);
    let stats = d.path().join("stats");
    fs::create_dir(&stats).unwrap();
    let wireless = d.path().join("wireless");
    fs::write(&wireless, WIRELESS).unwrap();
    a.set_net_meter(NetMeter::at(&stats, &wireless, "wlan0"));
    counters(&stats, 0, 0);
    a.apply(Action::QuickMenu);
    assert!(a.quick_menu().is_some(), "{:?}", a.phase());
    let t = a.now();
    a.tick_ms(t + 1);
    counters(&stats, 8_000, 1_000);
    a.tick_ms(t + 1 + NET_POLL_MS);
    assert_eq!(a.net_rate(), Some((8_000, 1_000)));
    assert_eq!(a.status_icons()[1], (StatusIcon::Wifi3, true));

    a.apply(Action::QuickMenu);
    assert_eq!(a.quick_menu(), None);
    assert_eq!(a.net_rate(), None, "closed menu, no meter");
}

#[test]
fn the_wifi_row_names_the_joined_network() {
    let (d, mut a) = booted(true, WifiState::Online);
    let ssid = d.path().join("ssid");
    fs::write(&ssid, "Home Net\n").unwrap();
    let gone = d.path().join("gone");
    a.set_net_meter(NetMeter::at(&gone, &gone, "wlan0").with_ssid_file(&ssid));
    a.apply(Action::QuickMenu);
    assert_eq!(
        a.quick_live_text(QuickRow::Wifi).as_deref(),
        Some("Connected")
    );
    let t = a.now();
    a.tick_ms(t + 1);
    assert_eq!(
        a.quick_live_text(QuickRow::Wifi).as_deref(),
        Some("Home Net")
    );
    assert_eq!(
        a.quick_live_text(QuickRow::Bluetooth).as_deref(),
        Some("Off")
    );
}

#[test]
fn the_wifi_row_says_why_it_is_not_online() {
    let (_d, a) = booted(true, WifiState::NotFound);
    assert_eq!(
        a.quick_live_text(QuickRow::Wifi).as_deref(),
        Some("Not Found")
    );
    let (_d, a) = booted(false, WifiState::Off);
    assert_eq!(a.quick_live_text(QuickRow::Wifi).as_deref(), Some("Off"));
}
