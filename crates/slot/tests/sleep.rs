mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::{boot, tmp_root_with_carts};
use slot::app::{App, Phase};
use slot_input::Action;
use slot_power::{Battery, Charge, LedState, Platform, Power};

/// A handheld whose sleep is a counter and whose clock the test moves; each
/// sleep lets `slept_s` pass on the wall clock.
struct Sleeper {
    root: PathBuf,
    sleeps: Arc<AtomicU32>,
    wall: Arc<AtomicI64>,
    slept_s: i64,
    can_sleep: bool,
}

impl Platform for Sleeper {
    fn set_backlight(&mut self, _step: u8) {}
    fn battery(&self) -> Option<Battery> {
        None
    }
    fn charge(&self) -> Charge {
        Charge::Discharging
    }
    fn set_led(&mut self, _state: LedState) {}
    fn poweroff(&mut self) -> ! {
        panic!("powered off")
    }
    fn restart(&mut self) -> ! {
        panic!("restarted")
    }
    fn root(&self) -> &Path {
        &self.root
    }
    fn now(&self) -> i64 {
        self.wall.load(Ordering::Relaxed)
    }
    fn set_clock(&mut self, _secs: i64) {}
    fn set_rumble(&mut self, _strength: u16) {}
    fn suspend(&mut self, wake_after_s: u32) -> bool {
        if !self.can_sleep {
            return false;
        }
        assert!(wake_after_s > 0);
        self.sleeps.fetch_add(1, Ordering::Relaxed);
        self.wall.fetch_add(self.slept_s, Ordering::Relaxed);
        true
    }
}

fn rig(root: &Path, slept_s: i64, can_sleep: bool) -> (App, Arc<AtomicU32>) {
    let mut a = boot(root);
    let sleeps = Arc::new(AtomicU32::new(0));
    a.set_power(Power::new(
        Box::new(Sleeper {
            root: root.to_path_buf(),
            sleeps: sleeps.clone(),
            wall: Arc::new(AtomicI64::new(1_786_568_000)),
            slept_s,
            can_sleep,
        }),
        Duration::from_secs(180),
    ));
    (a, sleeps)
}

#[test]
fn a_shut_lid_sleeps_in_ram_after_a_moment_and_again_after_a_stray_wake() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let (mut a, sleeps) = rig(d.path(), 60, true);
    a.apply(Action::LidClose);
    a.tick_ms(1_000);
    assert_eq!(
        sleeps.load(Ordering::Relaxed),
        0,
        "not before the lid has settled"
    );
    a.tick_ms(3_500);
    assert_eq!(sleeps.load(Ordering::Relaxed), 1);
    a.tick_ms(4_000);
    assert_eq!(sleeps.load(Ordering::Relaxed), 1, "a moment awake first");
    a.tick_ms(7_000);
    assert_eq!(sleeps.load(Ordering::Relaxed), 2, "the lid is still shut");
    a.tick_ms(400_000);
    assert!(
        matches!(a.phase(), Phase::Doze { .. }),
        "the short doze timeout no longer powers off once asleep"
    );
    a.apply(Action::LidOpen);
    assert!(!matches!(a.phase(), Phase::Doze { .. }));
    a.tick_ms(500_000);
    assert_eq!(sleeps.load(Ordering::Relaxed), 3, "awake with the lid open");
}

#[test]
fn a_lid_shut_for_hours_powers_off() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let (mut a, sleeps) = rig(d.path(), 7 * 60 * 60, true);
    a.apply(Action::LidClose);
    a.tick_ms(3_500);
    assert_eq!(sleeps.load(Ordering::Relaxed), 1);
    a.tick_ms(7_000);
    assert_eq!(
        sleeps.load(Ordering::Relaxed),
        1,
        "past the limit it does not sleep again"
    );
    assert!(a.powering_off());
}

#[test]
fn a_handheld_that_cannot_sleep_keeps_the_doze_timeout() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let (mut a, sleeps) = rig(d.path(), 60, false);
    a.apply(Action::LidClose);
    a.tick_ms(3_500);
    a.tick_ms(10_000);
    assert_eq!(sleeps.load(Ordering::Relaxed), 0);
    assert!(!a.powering_off());
    a.tick_ms(181_000);
    assert!(a.powering_off());
}
