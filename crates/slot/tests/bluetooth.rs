mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::bluetooth::{BtDevice, BtJob, BtJobs, BtState};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::{BtFound, BtSetup, QuickRow, QuickValue};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct Fake {
    asked: Arc<Mutex<Vec<BtJob>>>,
    state: Arc<Mutex<Option<BtState>>>,
    device: Arc<Mutex<Option<BtDevice>>>,
    scans: Arc<Mutex<usize>>,
    found: Arc<Mutex<Option<Vec<BtFound>>>>,
}

impl BtJobs for Fake {
    fn ask(&mut self, job: BtJob) {
        *self.state.lock().unwrap() = Some(BtState::during(&job));
        self.asked.lock().unwrap().push(job);
    }

    fn state(&self) -> BtState {
        self.state.lock().unwrap().unwrap_or(BtState::Off)
    }

    fn connected(&mut self) -> bool {
        self.device.lock().unwrap().is_some()
    }

    fn device(&self) -> Option<BtDevice> {
        self.device.lock().unwrap().clone()
    }

    fn scan(&mut self) {
        *self.scans.lock().unwrap() += 1;
        *self.found.lock().unwrap() = None;
    }

    fn scanned(&self) -> Option<Vec<BtFound>> {
        self.found.lock().unwrap().clone()
    }
}

const AIRPODS: &str = "AA:BB:CC:00:11:22";

fn nearby() -> Vec<BtFound> {
    vec![
        BtFound {
            mac: "AA:AA:AA:AA:AA:02".into(),
            name: "Far Speaker".into(),
            paired: false,
            signal: -40,
        },
        BtFound {
            mac: AIRPODS.into(),
            name: "My Headphones".into(),
            paired: true,
            signal: -60,
        },
    ]
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
    a.set_bt_jobs(Box::new(fake.clone()));
    (d, a, fake)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_bluetooth(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Bluetooth.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::BtPower));
}

fn choose_headphones(a: &mut App, fake: &Fake) {
    open_bluetooth(a);
    for _ in 0..QuickRow::BtHeadphones.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert_eq!(a.bt_setup(), Some(&BtSetup::Scanning));
    *fake.found.lock().unwrap() = Some(nearby());
    a.tick_ms(a.now() + 1);
    assert_eq!(a.bt_setup(), Some(&BtSetup::scanned(nearby())));
}

#[test]
fn bluetooth_starts_off_and_switching_it_on_wakes_the_radio_and_is_saved() {
    let (d, mut a, fake) = booted();
    open_bluetooth(&mut a);
    assert_eq!(a.quick_value(QuickRow::BtPower), Some(QuickValue::Off));
    press(&mut a, Btn::Right);
    assert_eq!(a.quick_value(QuickRow::BtPower), Some(QuickValue::On));
    assert!(read_slot_state(d.path()).bluetooth);
    press(&mut a, Btn::Left);
    assert!(!read_slot_state(d.path()).bluetooth);
    assert_eq!(*fake.asked.lock().unwrap(), [BtJob::On, BtJob::Off]);
}

#[test]
fn choosing_headphones_lists_what_the_scan_found_and_a_connects_the_picked_pair() {
    let (d, mut a, fake) = booted();
    choose_headphones(&mut a, &fake);
    assert!(
        read_slot_state(d.path()).bluetooth,
        "choosing headphones did not switch Bluetooth on"
    );
    assert_eq!(*fake.scans.lock().unwrap(), 1);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [BtJob::On, BtJob::Connect(AIRPODS.into())],
        "the radio was not switched on before the scan, or A did not connect the headphones under the cursor"
    );
    assert_eq!(
        a.bt_setup(),
        Some(&BtSetup::Connecting {
            name: "My Headphones".into()
        })
    );
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::Connecting)
    );
    *fake.state.lock().unwrap() = Some(BtState::Ready);
    a.tick_ms(a.now() + 1);
    assert_eq!(
        a.quick_menu(),
        Some(QuickRow::BtHeadphones),
        "a finished connect did not return to the Headphones row"
    );
}

#[test]
fn a_refused_pairing_is_explained_and_a_searches_again() {
    let (_d, mut a, fake) = booted();
    choose_headphones(&mut a, &fake);
    press(&mut a, Btn::A);
    *fake.state.lock().unwrap() = Some(BtState::Refused);
    a.tick_ms(a.now() + 1);
    assert_eq!(
        a.bt_setup(),
        Some(&BtSetup::Refused {
            name: "Far Speaker".into()
        })
    );
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::Refused)
    );
    press(&mut a, Btn::A);
    assert_eq!(a.bt_setup(), Some(&BtSetup::Scanning));
    assert_eq!(*fake.scans.lock().unwrap(), 2);
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::BtHeadphones));
}

#[test]
fn a_failed_connect_is_explained_and_b_goes_back_to_the_menu() {
    let (_d, mut a, fake) = booted();
    choose_headphones(&mut a, &fake);
    press(&mut a, Btn::A);
    *fake.state.lock().unwrap() = Some(BtState::Failed);
    a.tick_ms(a.now() + 1);
    assert_eq!(
        a.bt_setup(),
        Some(&BtSetup::Failed {
            name: "Far Speaker".into()
        })
    );
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::BtHeadphones));
    assert_eq!(a.bt_setup(), None);
}

#[test]
fn b_from_the_bluetooth_page_returns_to_its_row() {
    let (_d, mut a, _) = booted();
    open_bluetooth(&mut a);
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Bluetooth));
}

#[test]
fn connected_headphones_take_the_sound_and_show_their_name() {
    let (_d, mut a, fake) = booted();
    open_bluetooth(&mut a);
    press(&mut a, Btn::Right);
    *fake.state.lock().unwrap() = Some(BtState::Ready);
    *fake.device.lock().unwrap() = Some(BtDevice {
        mac: AIRPODS.into(),
        name: "My headphones".into(),
    });
    a.tick_ms(a.now() + 1_000);
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::Connected)
    );
    assert_eq!(a.bt_device_name().as_deref(), Some("My headphones"));
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert_eq!(
        a.bt_setup(),
        Some(&BtSetup::Scanning),
        "A on the Headphones row did not open the list"
    );
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [BtJob::On],
        "opening the list asked the radio for something, which can drop the stream"
    );
    press(&mut a, Btn::B);
    assert_eq!(
        slot::audio::route::bluetooth_device().as_deref(),
        Some(AIRPODS)
    );
    *fake.device.lock().unwrap() = None;
    a.tick_ms(a.now() + 1_000);
    assert_eq!(a.bt_device_name(), None);
    assert_eq!(slot::audio::route::bluetooth_device(), None);
}
