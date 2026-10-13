mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::wifi::{WifiJob, WifiJobs, WifiState, WIFI_CONF};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::{QuickRow, QuickValue, WifiNetwork, WifiSetup};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct Fake {
    asked: Arc<Mutex<Vec<WifiJob>>>,
    state: Arc<Mutex<Option<WifiState>>>,
    scans: Arc<Mutex<usize>>,
    found: Arc<Mutex<Option<Vec<WifiNetwork>>>>,
}

impl WifiJobs for Fake {
    fn ask(&mut self, job: WifiJob) {
        self.asked.lock().unwrap().push(job);
        *self.state.lock().unwrap() = Some(WifiState::during(job));
    }

    fn state(&self) -> WifiState {
        self.state.lock().unwrap().unwrap_or(WifiState::Off)
    }

    fn scan(&mut self) {
        *self.scans.lock().unwrap() += 1;
        *self.found.lock().unwrap() = None;
    }

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        self.found.lock().unwrap().clone()
    }
}

fn nearby() -> Vec<WifiNetwork> {
    vec![
        WifiNetwork {
            ssid: "Home".into(),
            secured: true,
            signal: -50,
        },
        WifiNetwork {
            ssid: "Cafe".into(),
            secured: false,
            signal: -80,
        },
    ]
}

fn choose_network(a: &mut App, fake: &Fake) {
    open_wifi(a);
    for _ in 0..QuickRow::WifiNetwork.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert_eq!(a.wifi_setup(), Some(&WifiSetup::Scanning));
    *fake.found.lock().unwrap() = Some(nearby());
    a.tick_ms(1);
    assert_eq!(a.wifi_setup(), Some(&WifiSetup::scanned(nearby())));
}

fn booted() -> (TempDir, App, Fake) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let fake = Fake::default();
    a.set_wifi_jobs(Box::new(fake.clone()));
    (d, a, fake)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_wifi(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Wifi.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiPower));
}

#[test]
fn wifi_starts_off_and_switching_it_on_joins_and_is_saved() {
    let (d, mut a, fake) = booted();
    open_wifi(&mut a);
    assert_eq!(a.quick_value(QuickRow::WifiPower), Some(QuickValue::Off));
    assert_eq!(a.quick_value(QuickRow::WifiNetwork), Some(QuickValue::Off));
    press(&mut a, Btn::Right);
    assert_eq!(a.quick_value(QuickRow::WifiPower), Some(QuickValue::On));
    assert_eq!(
        a.quick_value(QuickRow::WifiNetwork),
        Some(QuickValue::Joining)
    );
    assert!(read_slot_state(d.path()).wifi);
    press(&mut a, Btn::Left);
    assert!(!read_slot_state(d.path()).wifi);
    assert_eq!(*fake.asked.lock().unwrap(), [WifiJob::On, WifiJob::Off]);
}

#[test]
fn the_network_row_says_how_joining_went() {
    let (_d, mut a, fake) = booted();
    open_wifi(&mut a);
    press(&mut a, Btn::Right);
    for (state, shown) in [
        (WifiState::Online, QuickValue::Online),
        (WifiState::NoSetup, QuickValue::NoSetup),
        (WifiState::NotFound, QuickValue::NotFound),
        (WifiState::NoAddress, QuickValue::NoAddress),
        (WifiState::Failed, QuickValue::Trouble),
    ] {
        *fake.state.lock().unwrap() = Some(state);
        assert_eq!(
            a.quick_value(QuickRow::WifiNetwork),
            Some(shown),
            "{state:?}"
        );
    }
}

#[test]
fn a_saved_network_joins_again_without_its_password() {
    let (d, mut a, fake) = booted();
    std::fs::write(
        d.path().join(WIFI_CONF),
        "ssid=Cafe\n\nssid=Home\npsk=secret99\n",
    )
    .unwrap();
    open_wifi(&mut a);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    *fake.found.lock().unwrap() = Some(nearby());
    a.tick_ms(1);
    assert_eq!(
        a.wifi_setup(),
        Some(&WifiSetup::scanned_with(
            nearby(),
            vec!["Cafe".into(), "Home".into()]
        ))
    );
    press(&mut a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetwork));
    assert_eq!(*fake.asked.lock().unwrap(), [WifiJob::On]);
    let conf = std::fs::read_to_string(d.path().join(WIFI_CONF)).unwrap();
    assert!(
        conf.find("ssid=Home\npsk=secret99\n") < conf.find("ssid=Cafe"),
        "{conf}"
    );
}

#[test]
fn y_on_a_saved_network_asks_for_a_new_password() {
    let (d, mut a, fake) = booted();
    std::fs::write(d.path().join(WIFI_CONF), "ssid=Home\npsk=secret99\n").unwrap();
    open_wifi(&mut a);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    *fake.found.lock().unwrap() = Some(nearby());
    a.tick_ms(1);
    press(&mut a, Btn::Y);
    assert_eq!(a.wifi_setup(), Some(&WifiSetup::password("Home")));
    assert!(fake.asked.lock().unwrap().is_empty());
}

#[test]
fn b_from_the_wifi_page_returns_to_its_row() {
    let (_d, mut a, _) = booted();
    open_wifi(&mut a);
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Wifi));
}

#[test]
fn a_network_picked_and_typed_on_the_handheld_is_saved_and_joined() {
    let (d, mut a, fake) = booted();
    choose_network(&mut a, &fake);
    press(&mut a, Btn::A);
    press(&mut a, Btn::Down);
    for _ in 0..8 {
        press(&mut a, Btn::A);
        press(&mut a, Btn::Right);
    }
    press(&mut a, Btn::Start);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetwork));
    assert_eq!(*fake.asked.lock().unwrap(), [WifiJob::On]);
    assert!(read_slot_state(d.path()).wifi);
    let conf = std::fs::read_to_string(d.path().join(WIFI_CONF)).unwrap();
    assert!(conf.contains("ssid=Home\npsk=qwertyui\n"), "{conf}");
}

#[test]
fn an_open_network_joins_with_no_password() {
    let (d, mut a, fake) = booted();
    choose_network(&mut a, &fake);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert_eq!(*fake.asked.lock().unwrap(), [WifiJob::On]);
    let conf = std::fs::read_to_string(d.path().join(WIFI_CONF)).unwrap();
    assert!(
        conf.contains("ssid=Cafe\n") && !conf.contains("psk="),
        "{conf}"
    );
}

#[test]
fn x_scans_again_and_b_or_menu_leave_the_list() {
    let (_d, mut a, fake) = booted();
    choose_network(&mut a, &fake);
    press(&mut a, Btn::X);
    assert_eq!(*fake.scans.lock().unwrap(), 2);
    assert_eq!(a.wifi_setup(), Some(&WifiSetup::Scanning));
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetwork));
    press(&mut a, Btn::A);
    a.apply(Action::QuickMenu);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetwork));
    assert!(fake.asked.lock().unwrap().is_empty());
}
