mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::bluetooth::{BtDevice, BtJob, BtJobs, BtState};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::{QuickRow, QuickValue};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct Fake {
    asked: Arc<Mutex<Vec<BtJob>>>,
    state: Arc<Mutex<Option<BtState>>>,
    device: Arc<Mutex<Option<BtDevice>>>,
}

impl BtJobs for Fake {
    fn ask(&mut self, job: BtJob) {
        self.asked.lock().unwrap().push(job);
        *self.state.lock().unwrap() = Some(BtState::during(job));
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
fn pairing_switches_bluetooth_on_and_says_it_is_searching() {
    let (d, mut a, fake) = booted();
    open_bluetooth(&mut a);
    press(&mut a, Btn::Down);
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::NotConnected)
    );
    press(&mut a, Btn::A);
    assert!(read_slot_state(d.path()).bluetooth);
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::Searching)
    );
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [BtJob::Pair],
        "a second press while searching started another search"
    );
    *fake.state.lock().unwrap() = Some(BtState::NoneFound);
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::NoneFound)
    );
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
        mac: "30:7A:D2:85:D8:1D".into(),
        name: "Le battonetes 2".into(),
    });
    a.tick_ms(a.now() + 1_000);
    assert_eq!(
        a.quick_value(QuickRow::BtHeadphones),
        Some(QuickValue::Connected)
    );
    assert_eq!(a.bt_device_name().as_deref(), Some("Le battonetes 2"));
    assert_eq!(
        slot::audio::route::bluetooth_device().as_deref(),
        Some("30:7A:D2:85:D8:1D")
    );
    *fake.device.lock().unwrap() = None;
    a.tick_ms(a.now() + 1_000);
    assert_eq!(a.bt_device_name(), None);
    assert_eq!(slot::audio::route::bluetooth_device(), None);
}
