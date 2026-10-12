mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::diag::{DiagJob, DiagJobs, DiagState, Radios, CRASH_CAUGHT};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, Toast};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct FakeDiag(Arc<Mutex<(DiagState, Vec<DiagJob>)>>);

impl DiagJobs for FakeDiag {
    fn run(&mut self, job: DiagJob, _radios: Radios) {
        let mut s = self.0.lock().unwrap();
        s.0 = DiagState::Running(job);
        s.1.push(job);
    }

    fn state(&self) -> DiagState {
        self.0.lock().unwrap().0
    }
}

fn seated(cart: Option<&str>) -> SlotState {
    SlotState {
        cart: cart.map(Into::into),
        clock_set: true,
        ..SlotState::default()
    }
}

fn booted(carts: &[&str], cart: Option<&str>) -> (TempDir, App, FakeDiag) {
    let d = tmp_root_with_carts(carts);
    write_slot_state(d.path(), &seated(cart)).expect("write slot.state");
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let diag = FakeDiag::default();
    a.set_diag_jobs(Box::new(diag.clone()));
    (d, a, diag)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_update_row(a: &mut App, row: QuickRow) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Update.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    for _ in 0..row.position() {
        press(a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(row));
}

#[test]
fn the_bug_report_row_saves_a_report_once_per_press() {
    let (_d, mut a, diag) = booted(&["Emerald", "Fusion"], None);
    open_update_row(&mut a, QuickRow::BugReport);
    assert_eq!(
        a.quick_live_text(QuickRow::BugReport).as_deref(),
        Some("Save")
    );
    press(&mut a, Btn::A);
    press(&mut a, Btn::A);
    assert_eq!(diag.0.lock().unwrap().1, [DiagJob::BugReport]);
    assert_eq!(
        a.quick_live_text(QuickRow::BugReport).as_deref(),
        Some("Saving...")
    );
    diag.0.lock().unwrap().0 = DiagState::Saved;
    assert_eq!(
        a.quick_live_text(QuickRow::BugReport).as_deref(),
        Some("Saved in Reports")
    );
}

#[test]
fn the_self_test_row_runs_the_test_and_shows_what_it_found() {
    let (_d, mut a, diag) = booted(&["Emerald", "Fusion"], None);
    open_update_row(&mut a, QuickRow::SelfTest);
    press(&mut a, Btn::A);
    assert_eq!(diag.0.lock().unwrap().1, [DiagJob::SelfTest]);
    diag.0.lock().unwrap().0 = DiagState::Tested(2);
    assert_eq!(
        a.quick_live_text(QuickRow::SelfTest).as_deref(),
        Some("2 Problems")
    );
}

#[test]
fn a_new_version_tests_itself_and_speaks_up_only_about_problems() {
    let (_d, mut a, diag) = booted(&["Emerald", "Fusion"], None);
    a.tick_ms(16_000);
    assert_eq!(diag.0.lock().unwrap().1, [DiagJob::SelfTestOnce]);
    diag.0.lock().unwrap().0 = DiagState::Tested(1);
    a.tick_ms(16_100);
    assert_eq!(a.toast(), Some(Toast::SelfTestProblems));

    let (_d, mut a, diag) = booted(&["Emerald", "Fusion"], None);
    a.tick_ms(16_000);
    diag.0.lock().unwrap().0 = DiagState::Tested(0);
    a.tick_ms(16_100);
    assert_eq!(a.toast(), None);
}

#[test]
fn after_a_crash_the_game_stays_on_the_shelf_and_slot_says_why() {
    let d = tmp_root_with_carts(&["Emerald"]);
    write_slot_state(d.path(), &seated(Some("Emerald"))).unwrap();
    std::fs::create_dir_all(d.path().join("crash")).unwrap();
    std::fs::write(d.path().join(CRASH_CAUGHT), "2026-10-12_21-00-00\n").unwrap();
    let a = App::boot(d.path());
    assert!(
        matches!(a.phase(), Phase::Shelf),
        "a crashed game must not be put straight back in"
    );
    assert_eq!(a.toast(), Some(Toast::CrashCaught));
    assert!(!d.path().join(CRASH_CAUGHT).exists());

    let a = App::boot(d.path());
    assert!(matches!(a.phase(), Phase::Inserting { .. }));
    assert_eq!(a.toast(), None);
}
