mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::update::{Releases, Step, UpdateJob, UpdateJobs, UpdateState};
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, WifiNetwork};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct FakeUpdate {
    asked: Arc<Mutex<Vec<UpdateJob>>>,
    state: Arc<Mutex<Option<UpdateState>>>,
    confirmed: Arc<Mutex<usize>>,
}

impl UpdateJobs for FakeUpdate {
    fn ask(&mut self, job: UpdateJob) {
        self.asked.lock().unwrap().push(job);
        *self.state.lock().unwrap() = Some(match job {
            UpdateJob::Check => UpdateState::Checking,
            _ => UpdateState::Downloading(job, Step::Started),
        });
    }

    fn state(&self) -> UpdateState {
        self.state
            .lock()
            .unwrap()
            .clone()
            .unwrap_or(UpdateState::Idle)
    }

    fn confirm(&mut self) {
        *self.confirmed.lock().unwrap() += 1;
    }
}

#[derive(Clone)]
struct OnlineWifi(WifiState);

impl WifiJobs for OnlineWifi {
    fn ask(&mut self, _job: WifiJob) {}

    fn state(&self) -> WifiState {
        self.0
    }

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        Some(Vec::new())
    }
}

fn booted(wifi: WifiState) -> (TempDir, App, FakeUpdate) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    std::fs::write(d.path().join("System/release"), "v1.7.0\n").expect("write release");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let fake = FakeUpdate::default();
    a.set_update_jobs(Box::new(fake.clone()));
    a.set_wifi_jobs(Box::new(OnlineWifi(wifi)));
    (d, a, fake)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_update(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Update.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::SlotUpdate));
}

fn checked(slot_latest: &str, baseos_latest: &str) -> UpdateState {
    UpdateState::Checked(Releases {
        slot_installed: "v1.7.0".into(),
        slot_latest: slot_latest.into(),
        baseos_installed: "1.3.0".into(),
        baseos_latest: baseos_latest.into(),
        baseos_url: "https://dl/baseos-rg34xxsp.bosupd".into(),
    })
}

#[test]
fn opening_the_page_checks_for_updates_when_online() {
    let (_d, mut a, fake) = booted(WifiState::Online);
    open_update(&mut a);
    assert_eq!(*fake.asked.lock().unwrap(), [UpdateJob::Check]);
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("v1.7.0, Checking...")
    );
    *fake.state.lock().unwrap() = Some(checked("v1.8.0", "1.3.0"));
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("v1.7.0, Get v1.8.0")
    );
    assert_eq!(
        a.quick_live_text(QuickRow::BaseOsUpdate).as_deref(),
        Some("1.3.0 Up to Date")
    );
}

#[test]
fn with_wifi_off_the_page_asks_for_it_and_does_nothing_else() {
    let (_d, mut a, fake) = booted(WifiState::Off);
    open_update(&mut a);
    press(&mut a, Btn::A);
    assert!(fake.asked.lock().unwrap().is_empty());
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("v1.7.0, Wi-Fi Off")
    );
}

#[test]
fn a_on_an_available_release_downloads_it_and_then_restarts_to_install() {
    let (_d, mut a, fake) = booted(WifiState::Online);
    open_update(&mut a);
    *fake.state.lock().unwrap() = Some(checked("v1.8.0", "1.4.0"));
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [UpdateJob::Check, UpdateJob::Slot]
    );
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("Downloading...")
    );
    *fake.state.lock().unwrap() =
        Some(UpdateState::Downloading(UpdateJob::Slot, Step::Percent(42)));
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("Downloading 42%...")
    );
    *fake.state.lock().unwrap() = Some(UpdateState::Downloading(UpdateJob::Slot, Step::Verifying));
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("Verifying...")
    );
    // BaseOS's version comes from /etc/baseos-release, which a test host lacks.
    assert!(
        a.quick_live_text(QuickRow::BaseOsUpdate)
            .unwrap()
            .ends_with(", Busy"),
        "BaseOS was not shown as waiting while slot downloads"
    );
    press(&mut a, Btn::A);
    assert_eq!(
        fake.asked.lock().unwrap().len(),
        2,
        "a press while downloading started another download"
    );
    *fake.state.lock().unwrap() = Some(UpdateState::Ready(UpdateJob::Slot));
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("Restart to Install")
    );
    assert!(!a.restarting());
    press(&mut a, Btn::A);
    assert!(a.restarting(), "A on a ready update did not restart");
}

#[test]
fn an_up_to_date_release_is_not_downloaded_and_a_failure_offers_a_recheck() {
    let (_d, mut a, fake) = booted(WifiState::Online);
    open_update(&mut a);
    *fake.state.lock().unwrap() = Some(checked("v1.7.0", "1.4.0"));
    press(&mut a, Btn::A);
    assert_eq!(*fake.asked.lock().unwrap(), [UpdateJob::Check]);
    press(&mut a, Btn::Down);
    assert_eq!(a.quick_menu(), Some(QuickRow::BaseOsUpdate));
    assert_eq!(
        a.quick_live_text(QuickRow::BaseOsUpdate).as_deref(),
        Some("1.3.0, Get 1.4.0")
    );
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [UpdateJob::Check, UpdateJob::BaseOs]
    );
    *fake.state.lock().unwrap() = Some(UpdateState::Failed(UpdateJob::BaseOs, Some(4)));
    assert_eq!(
        a.quick_live_text(QuickRow::BaseOsUpdate).as_deref(),
        Some("Failed, Retry")
    );
    press(&mut a, Btn::A);
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [UpdateJob::Check, UpdateJob::BaseOs, UpdateJob::Check]
    );
}

#[test]
fn a_swapped_in_system_is_kept_once_slot_has_run_a_while() {
    let (_d, mut a, fake) = booted(WifiState::Off);
    a.tick_ms(1_000);
    assert_eq!(*fake.confirmed.lock().unwrap(), 0);
    a.tick_ms(20_000);
    assert_eq!(*fake.confirmed.lock().unwrap(), 1);
    a.tick_ms(40_000);
    assert_eq!(*fake.confirmed.lock().unwrap(), 1, "confirmed twice");
}

#[test]
fn a_downloaded_update_installs_with_wifi_gone_and_survives_reopening_the_page() {
    let (_d, mut a, fake) = booted(WifiState::Online);
    open_update(&mut a);
    *fake.state.lock().unwrap() = Some(UpdateState::Ready(UpdateJob::Slot));
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Update));
    press(&mut a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::SlotUpdate));
    assert_eq!(
        *fake.asked.lock().unwrap(),
        [UpdateJob::Check],
        "reopening the page checked again and forgot the download"
    );
    a.set_wifi_jobs(Box::new(OnlineWifi(WifiState::Off)));
    assert_eq!(
        a.quick_live_text(QuickRow::SlotUpdate).as_deref(),
        Some("Restart to Install")
    );
    press(&mut a, Btn::A);
    assert!(a.restarting(), "a ready update needed Wi-Fi to install");
}
