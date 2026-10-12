mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::drive::{DriveJobs, DriveState};
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, WifiNetwork};
use tempfile::TempDir;

#[derive(Clone)]
struct FakeDrive(Arc<Mutex<DriveState>>);

impl DriveJobs for FakeDrive {
    fn start(&mut self) {
        *self.0.lock().unwrap() = DriveState::On;
    }

    fn stop(&mut self) {
        *self.0.lock().unwrap() = DriveState::Off;
    }

    fn state(&self) -> DriveState {
        *self.0.lock().unwrap()
    }
}

#[derive(Clone)]
struct FakeWifi(Arc<Mutex<WifiState>>);

impl WifiJobs for FakeWifi {
    fn ask(&mut self, job: WifiJob) {
        *self.0.lock().unwrap() = match job {
            WifiJob::On => WifiState::Online,
            _ => WifiState::Off,
        };
    }

    fn state(&self) -> WifiState {
        *self.0.lock().unwrap()
    }

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        Some(Vec::new())
    }
}

struct Rig {
    d: TempDir,
    a: App,
    drive: FakeDrive,
    now: u64,
}

fn booted(wifi: WifiState) -> Rig {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            wifi: wifi == WifiState::Online,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let drive = FakeDrive(Arc::new(Mutex::new(DriveState::Off)));
    a.set_drive_jobs(Box::new(drive.clone()));
    a.set_wifi_jobs(Box::new(FakeWifi(Arc::new(Mutex::new(wifi)))));
    Rig {
        d,
        a,
        drive,
        now: 0,
    }
}

impl Rig {
    fn press(&mut self, btn: Btn) {
        self.a.apply(Action::GbaDown(btn));
        self.a.apply(Action::GbaUp(btn));
    }

    fn open_drive_row(&mut self) {
        self.a.apply(Action::QuickMenu);
        for _ in 0..QuickRow::Wifi.position() {
            self.press(Btn::Down);
        }
        self.press(Btn::A);
        for _ in 0..QuickRow::WifiDrive.position() {
            self.press(Btn::Down);
        }
        assert_eq!(self.a.quick_menu(), Some(QuickRow::WifiDrive));
    }

    fn wait(&mut self, ms: u64) {
        let end = self.now + ms;
        while self.now < end {
            self.now += 500;
            self.a.tick_ms(self.now);
        }
    }

    fn stems(&self) -> Vec<String> {
        self.a.carts().map(|c| c.stem.clone()).collect()
    }

    fn drop_rom(&self, stem: &str) {
        let rom = self.d.path().join(format!("Games/GBA/{stem}.gba"));
        std::fs::write(rom, vec![0u8; 0x100]).expect("write rom");
    }

    fn text(&self) -> Option<String> {
        self.a.quick_live_text(QuickRow::WifiDrive)
    }
}

#[test]
fn the_drive_waits_for_wifi() {
    let mut r = booted(WifiState::Off);
    r.open_drive_row();
    assert_eq!(r.text().as_deref(), Some("Needs Wi-Fi"));
    r.press(Btn::A);
    assert_eq!(r.drive.state(), DriveState::Off);
}

#[test]
fn turning_it_on_shows_where_to_find_it() {
    let mut r = booted(WifiState::Online);
    r.open_drive_row();
    assert_eq!(r.text().as_deref(), Some("Off"));
    r.press(Btn::Right);
    assert_eq!(r.drive.state(), DriveState::On);
    assert!(r.text().unwrap().ends_with(".local"), "{:?}", r.text());
    r.press(Btn::A);
    assert_eq!(r.drive.state(), DriveState::Off);
}

#[test]
fn a_game_dropped_on_while_it_is_on_joins_the_shelf_once_it_has_landed() {
    let mut r = booted(WifiState::Online);
    r.open_drive_row();
    r.press(Btn::A);
    r.press(Btn::B);
    r.press(Btn::B);
    assert_eq!(r.a.quick_menu(), None);
    r.drop_rom("Crystal");
    r.wait(3_000);
    assert_eq!(r.stems(), ["Emerald", "Fusion"], "still arriving");
    r.wait(3_000);
    assert_eq!(r.stems(), ["Crystal", "Emerald", "Fusion"]);
}

#[test]
fn turning_it_off_picks_up_what_arrived() {
    let mut r = booted(WifiState::Online);
    r.open_drive_row();
    r.press(Btn::A);
    r.drop_rom("Crystal");
    r.press(Btn::A);
    assert_eq!(r.drive.state(), DriveState::Off);
    assert_eq!(r.stems(), ["Crystal", "Emerald", "Fusion"]);
}

#[test]
fn turning_wifi_off_stops_the_drive() {
    let mut r = booted(WifiState::Online);
    r.open_drive_row();
    r.press(Btn::A);
    while r.a.quick_menu() != Some(QuickRow::WifiPower) {
        r.press(Btn::Up);
    }
    r.press(Btn::Left);
    assert_eq!(r.drive.state(), DriveState::Off);
}
