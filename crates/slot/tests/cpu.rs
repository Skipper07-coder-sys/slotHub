mod common;

use common::{boot, tmp_root_with_carts};
use slot::app::Phase;
use slot_input::Action;
use slot_power::CpuMode;
use slot_ui::FfState;

#[test]
fn the_cpu_follows_the_load_and_holds_the_top_clock_only_for_fast_forward() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    assert_eq!(a.cpu_mode(), CpuMode::Balanced);
    a.set_ff(FfState::Held);
    assert_eq!(a.cpu_mode(), CpuMode::Full);
    a.set_ff(FfState::Latched);
    assert_eq!(a.cpu_mode(), CpuMode::Full);
    a.set_ff(FfState::Off);
    assert_eq!(a.cpu_mode(), CpuMode::Balanced);
}

#[test]
fn a_shut_lid_rests_the_cpu_and_opening_it_brings_it_back() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    a.apply(Action::LidClose);
    assert_eq!(a.cpu_mode(), CpuMode::Rest);
    a.apply(Action::LidOpen);
    assert_eq!(a.cpu_mode(), CpuMode::Balanced);
}

#[test]
fn a_running_game_holds_the_top_clock_and_the_shelf_gives_it_back() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = boot(d.path());
    a.apply(Action::Insert);
    a.on_core_ready();
    assert_eq!(a.cpu_mode(), CpuMode::Full);
    while !matches!(a.phase(), Phase::Playing { .. }) {
        a.update(1.0 / 60.0);
    }
    assert_eq!(a.cpu_mode(), CpuMode::Full);
    a.apply(Action::Eject);
    while !matches!(a.phase(), Phase::Shelf) {
        a.update(1.0 / 60.0);
    }
    assert_eq!(a.cpu_mode(), CpuMode::Balanced);
}
