mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, SlotState};
use slot_ui::{QuickRow, QuickValue};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct Fake {
    asked: Arc<Mutex<Vec<WifiJob>>>,
    state: Arc<Mutex<Option<WifiState>>>,
}

impl WifiJobs for Fake {
    fn ask(&mut self, job: WifiJob) {
        self.asked.lock().unwrap().push(job);
        *self.state.lock().unwrap() = Some(WifiState::during(job));
    }

    fn state(&self) -> WifiState {
        self.state.lock().unwrap().unwrap_or(WifiState::Off)
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
fn a_on_the_network_row_tries_again_but_not_while_joining_or_off() {
    let (_d, mut a, fake) = booted();
    open_wifi(&mut a);
    press(&mut a, Btn::Down);
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiNetwork));
    press(&mut a, Btn::A);
    assert!(fake.asked.lock().unwrap().is_empty(), "joined while off");
    press(&mut a, Btn::Up);
    press(&mut a, Btn::Right);
    press(&mut a, Btn::Down);
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [WifiJob::On],
        "a second join started while the first was still joining"
    );
    *fake.state.lock().unwrap() = Some(WifiState::NotFound);
    press(&mut a, Btn::A);
    assert_eq!(*fake.asked.lock().unwrap(), [WifiJob::On, WifiJob::On]);
}

#[test]
fn b_from_the_wifi_page_returns_to_its_row() {
    let (_d, mut a, _) = booted();
    open_wifi(&mut a);
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Wifi));
}
