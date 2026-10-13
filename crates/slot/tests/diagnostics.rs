mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::diag::{DiagJob, DiagJobs, DiagState, Radios, CRASH_CAUGHT};
use slot_store::{write_slot_state, SlotState};
use slot_ui::Toast;
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

#[test]
fn a_new_version_tests_itself_quietly() {
    let (_d, mut a, diag) = booted(&["Emerald", "Fusion"], None);
    a.tick_ms(16_000);
    assert_eq!(diag.0.lock().unwrap().1, [DiagJob::SelfTestOnce]);
    diag.0.lock().unwrap().0 = DiagState::Tested(1);
    a.tick_ms(16_100);
    assert_eq!(
        a.toast(),
        None,
        "the self-test report goes to Reports/, not on screen"
    );
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
