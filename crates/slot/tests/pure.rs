mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use common::{
    app_booting_at, app_playing_in, app_playing_with, clocked, tmp_root_with_carts, StubSnapshot,
    CLOCK_IS_SET,
};
use slot::app::{App, Phase};
use slot::emu::{EmuHandle, Speed};
use slot::persist;
use slot::session::Session;
use slot_input::{Action, Btn, RawEvent};
use slot_retro::ButtonMask;
use slot_store::{
    read_slot_state, write_slot_state, Core, Platform, SlotState, StateRing, PURE_FILE,
};
use slot_ui::{edge, Draw, QuickMenuFaces, QuickRow, QuickValue, TexId, OUT_H, OUT_W, QUICK_PITCH};
use tempfile::TempDir;

const FRAME: f32 = 1.0 / 60.0;

fn make_pure(root: &Path) {
    std::fs::write(root.join(PURE_FILE), "pure = on\n").expect("write pure.txt");
}

fn pure_root(stems: &[&str]) -> TempDir {
    let d = tmp_root_with_carts(stems);
    make_pure(d.path());
    d
}

fn ring(root: &Path, stem: &str) -> StateRing {
    StateRing::new(root, Platform::Gba, Core::Mgba, stem)
}

fn resume(root: &Path, stem: &str) -> Option<Vec<u8>> {
    ring(root, stem).read_resume().expect("read resume")
}

fn frames(a: &mut App, n: usize) {
    for _ in 0..n {
        a.update(FRAME);
    }
}

fn seat_again(a: &mut App) {
    a.set_snapshot(StubSnapshot::boxed());
    a.on_core_ready();
    frames(a, 120);
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "the cart never seated: {:?}",
        a.phase()
    );
}

fn eject_to_the_shelf(a: &mut App) {
    a.apply(Action::Eject);
    frames(a, 240);
    assert!(
        matches!(a.phase(), Phase::Shelf),
        "the cart never came out: {:?}",
        a.phase()
    );
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

#[test]
fn the_file_on_the_card_is_what_turns_pure_on() {
    let d = tmp_root_with_carts(&["Emerald"]);
    assert!(!app_playing_in(d.path(), "Emerald").pure());
    make_pure(d.path());
    assert!(app_playing_in(d.path(), "Emerald").pure());
    std::fs::write(d.path().join(PURE_FILE), "pure = off\n").unwrap();
    assert!(!app_playing_in(d.path(), "Emerald").pure());
}

#[test]
fn pure_overrules_the_settings_that_would_allow_each_shortcut() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let a = app_playing_in(d.path(), "Emerald");
    assert!(
        a.may_fast_forward() && a.may_rewind() && a.records_rewind() && a.turbo(),
        "the stock settings do not allow what this test takes away"
    );
    assert!(a.may_load_state());

    make_pure(d.path());
    let a = app_playing_in(d.path(), "Emerald");
    assert!(!a.may_fast_forward(), "fast forward");
    assert!(!a.may_rewind(), "rewind");
    assert!(!a.records_rewind(), "rewind history is still recorded");
    assert!(!a.turbo(), "turbo buttons");
    assert!(!a.may_load_state(), "loading a state");
}

#[test]
fn pure_leaves_the_stored_settings_alone_for_the_day_it_is_switched_off() {
    let d = pure_root(&["Emerald"]);
    let _a = app_playing_in(d.path(), "Emerald");
    let s = read_slot_state(d.path());
    assert!(s.turbo && s.rewind, "pure rewrote the settings it hides");
    assert_eq!(s.ff_speed, SlotState::default().ff_speed);
}

#[test]
fn saving_a_state_writes_nothing_and_says_nothing() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::SaveState);
    assert!(
        ring(d.path(), "Emerald").list().unwrap().is_empty(),
        "a state was saved"
    );
    assert_eq!(a.toast(), None, "it claimed to have saved");
    assert!(!a.undo_available(a.now()), "there is a save to undo");
}

#[test]
fn loading_does_nothing_even_with_states_already_on_the_card() {
    let d = pure_root(&["Emerald"]);
    ring(d.path(), "Emerald")
        .push(&[5u8; 16], b"png", "2026-08-09_00-00-00")
        .unwrap();
    let (snapshot, loaded) = StubSnapshot::pair();
    let mut a = app_playing_with(d.path(), "Emerald", snapshot);

    a.apply(Action::LoadState);
    assert_eq!(*loaded.lock().unwrap(), None, "the state was loaded");

    a.apply(Action::Polaroids);
    assert!(
        matches!(a.phase(), Phase::Playing { .. }),
        "the state switcher opened: {:?}",
        a.phase()
    );
    assert_eq!(
        ring(d.path(), "Emerald").list().unwrap().len(),
        1,
        "pure threw away a state the player had saved before"
    );
}

#[test]
fn the_same_keys_still_work_on_a_card_without_the_file() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let (snapshot, loaded) = StubSnapshot::pair();
    let mut a = app_playing_with(d.path(), "Emerald", snapshot);
    a.apply(Action::SaveState);
    assert_eq!(ring(d.path(), "Emerald").list().unwrap().len(), 1);
    a.apply(Action::LoadState);
    assert!(
        loaded.lock().unwrap().is_some(),
        "a plain card lost loading"
    );
    a.apply(Action::Polaroids);
    assert!(matches!(a.phase(), Phase::Polaroids { .. }));
}

#[test]
fn a_locked_key_passes_without_the_refusal_shake() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    for action in [
        Action::FfStart,
        Action::FfStop,
        Action::RewindStart,
        Action::RewindStop,
        Action::SaveState,
        Action::LoadState,
        Action::Polaroids,
    ] {
        a.apply(action);
        assert!(
            !a.refusal_active(a.now()),
            "{action:?} shook the screen like a refused key"
        );
        assert!(matches!(a.phase(), Phase::Playing { .. }), "{action:?}");
    }
}

fn pure_carousel() -> (TempDir, App) {
    let d = pure_root(&["Emerald", "Fusion"]);
    clocked(d.path());
    let (a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    (d, a)
}

fn open_gameplay(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Game.position() {
        press(a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Game));
    press(a, Btn::A);
}

#[test]
fn the_gameplay_page_holds_only_rumble_and_auto_save_on_eject() {
    let (_d, mut a) = pure_carousel();
    open_gameplay(&mut a);
    assert_eq!(a.quick_menu(), Some(QuickRow::Rumble));
    let mut seen = vec![QuickRow::Rumble];
    for _ in 0..5 {
        press(&mut a, Btn::Down);
        seen.push(a.quick_menu().expect("the menu closed"));
    }
    assert_eq!(
        seen,
        [
            QuickRow::Rumble,
            QuickRow::EjectSave,
            QuickRow::Rumble,
            QuickRow::EjectSave,
            QuickRow::Rumble,
            QuickRow::EjectSave,
        ]
    );
    press(&mut a, Btn::Up);
    assert_eq!(a.quick_menu(), Some(QuickRow::Rumble));
    press(&mut a, Btn::Up);
    assert_eq!(
        a.quick_menu(),
        Some(QuickRow::EjectSave),
        "up from the top did not wrap to the bottom"
    );
    press(&mut a, Btn::B);
    assert_eq!(a.quick_menu(), Some(QuickRow::Game), "B lost its way back");
}

#[test]
fn the_rows_pure_keeps_still_change_and_save() {
    let (d, mut a) = pure_carousel();
    open_gameplay(&mut a);
    press(&mut a, Btn::Right);
    assert!(!read_slot_state(d.path()).rumble, "rumble never changed");
    press(&mut a, Btn::Down);
    press(&mut a, Btn::Left);
    let s = read_slot_state(d.path());
    assert!(!s.eject_save, "auto save on eject never changed");
    assert!(s.turbo && s.rewind, "a hidden row changed with them");
}

#[test]
fn the_screen_page_is_untouched() {
    let (_d, mut a) = pure_carousel();
    a.apply(Action::QuickMenu);
    assert_eq!(a.quick_menu(), Some(QuickRow::Screen));
    press(&mut a, Btn::A);
    let mut seen = Vec::new();
    for _ in 0..QuickRow::SCREEN.len() {
        seen.push(a.quick_menu().expect("the menu closed"));
        press(&mut a, Btn::Down);
    }
    assert_eq!(seen, QuickRow::SCREEN);
}

fn fake_faces(a: &mut App) {
    let id = TexId::from_raw;
    a.set_quick_menu_faces(QuickMenuFaces {
        labels: (0..QuickRow::ALL.len())
            .map(|i| (id(100 + i), 200, 40))
            .collect(),
        values: (0..QuickValue::ALL.len())
            .map(|i| [(id(200 + i), 60, 40), (id(210 + i), 60, 40)])
            .collect(),
        carets: [(id(300), 10, 40), (id(301), 10, 40)],
        legend: [(id(400), 70), (id(401), 110), (id(402), 80), (id(403), 80)],
    });
}

fn label_drawn(out: &[Draw], row: QuickRow) -> bool {
    out.iter()
        .any(|d| matches!(d, Draw::Tex { tex, .. } if *tex == TexId::from_raw(100 + row.index())))
}

#[test]
fn the_gameplay_page_draws_two_rows_centred_and_none_of_the_locked_ones() {
    let (_d, mut a) = pure_carousel();
    fake_faces(&mut a);
    open_gameplay(&mut a);
    let mut out = Vec::new();
    a.draw(&mut out);

    for row in QuickRow::GAME {
        assert_eq!(
            label_drawn(&out, row),
            QuickRow::GAME_PURE.contains(&row),
            "{row:?}"
        );
    }
    let bars: Vec<f32> = out
        .iter()
        .filter_map(|d| match d {
            Draw::Rect { y, w, colour, .. } if *colour == edge() && *w == OUT_W as f32 => Some(*y),
            _ => None,
        })
        .collect();
    let top = (OUT_H as f32 - QUICK_PITCH * 2.0) / 2.0;
    assert_eq!(
        bars,
        vec![top + 4.0],
        "the bar is not on the first of two rows"
    );
}

#[test]
fn a_suspend_on_the_card_is_spent_the_moment_play_begins() {
    let d = tmp_root_with_carts(&["Emerald"]);
    persist::flush(
        d.path(),
        Platform::Gba,
        Core::Mgba,
        "Emerald",
        Some(&[7u8; 64]),
        None,
    )
    .unwrap();
    let _a = app_playing_in(d.path(), "Emerald");
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "a plain card lost its resume by being played"
    );

    make_pure(d.path());
    let _a = app_playing_in(d.path(), "Emerald");
    assert_eq!(
        resume(d.path(), "Emerald"),
        None,
        "the suspend is still there to be loaded again"
    );
}

#[test]
fn closing_the_lid_suspends_and_opening_it_spends_the_suspend() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    assert_eq!(resume(d.path(), "Emerald"), None);

    a.apply(Action::LidClose);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the lid closed without a suspend"
    );
    frames(&mut a, 30);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the suspend was spent while the lid was still shut"
    );

    a.apply(Action::LidOpen);
    frames(&mut a, 1);
    assert_eq!(
        resume(d.path(), "Emerald"),
        None,
        "play carried on with the suspend still on the card"
    );
}

#[test]
fn a_press_of_power_alone_leaves_nothing_to_reload() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::PowerPress);
    frames(&mut a, 1);
    assert_eq!(resume(d.path(), "Emerald"), None);

    a.apply(Action::PowerTap);
    assert!(matches!(a.phase(), Phase::Doze { .. }));
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the screen went dark without a suspend"
    );

    a.apply(Action::PowerPress);
    a.apply(Action::PowerTap);
    assert!(matches!(a.phase(), Phase::Playing { .. }));
    frames(&mut a, 1);
    assert_eq!(resume(d.path(), "Emerald"), None);
}

#[test]
fn backing_out_of_the_power_menu_spends_the_suspend_it_took() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::PowerPress);
    a.apply(Action::PowerHold);
    assert_eq!(a.power_menu(), Some(0));
    frames(&mut a, 30);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the power menu is up with no suspend behind it"
    );

    press(&mut a, Btn::B);
    assert_eq!(a.power_menu(), None);
    frames(&mut a, 1);
    assert_eq!(resume(d.path(), "Emerald"), None);
}

#[test]
fn a_suspend_taken_under_the_link_menu_waits_for_the_menu_to_close() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::GameMenu);
    assert!(a.game_menu_open(), "the link menu never opened");
    a.flush_resume();
    frames(&mut a, 30);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the suspend was spent under an open link menu"
    );

    press(&mut a, Btn::B);
    assert!(!a.game_menu_open());
    frames(&mut a, 1);
    assert_eq!(resume(d.path(), "Emerald"), None);
}

#[test]
fn powering_off_keeps_the_suspend_for_exactly_one_boot() {
    for choice in 0..2 {
        let d = pure_root(&["Emerald"]);
        let mut a = app_playing_in(d.path(), "Emerald");
        a.apply(Action::PowerPress);
        a.apply(Action::PowerHold);
        for _ in 0..choice {
            press(&mut a, Btn::Down);
        }
        press(&mut a, Btn::A);
        assert!(a.shutting_down(), "choice {choice} did not shut down");
        frames(&mut a, 60);
        assert!(
            resume(d.path(), "Emerald").is_some(),
            "choice {choice} shut down with no suspend to come back to"
        );
        drop(a);

        let _a = app_playing_in(d.path(), "Emerald");
        assert_eq!(
            resume(d.path(), "Emerald"),
            None,
            "the boot after choice {choice} left the suspend to be used again"
        );
    }
}

#[test]
fn the_lid_timing_out_keeps_the_suspend() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::LidClose);
    a.on_doze_timeout();
    assert!(a.powering_off());
    frames(&mut a, 60);
    assert!(resume(d.path(), "Emerald").is_some());
}

#[test]
fn a_dying_battery_keeps_the_suspend() {
    let d = pure_root(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.on_battery(slot_power::Battery {
        percent: 3,
        charge: slot_power::Charge::Discharging,
    });
    assert!(a.powering_off());
    frames(&mut a, 60);
    assert!(resume(d.path(), "Emerald").is_some());
}

#[test]
fn the_minute_autosave_keeps_the_battery_save_and_writes_no_resume() {
    let playing = |root: &Path| {
        let stub = StubSnapshot {
            state: vec![9u8; 1024],
            sav: Some(b"battery".to_vec()),
            thumb: None,
            loaded: Default::default(),
        };
        app_playing_with(root, "Emerald", Box::new(stub))
    };
    let sav = |root: &Path| std::fs::read(root.join("Saves/GBA/Emerald.sav")).ok();

    let d = pure_root(&["Emerald"]);
    let mut a = playing(d.path());
    a.tick_ms(60_000);
    a.settle_saves();
    assert_eq!(
        sav(d.path()).as_deref(),
        Some(&b"battery"[..]),
        "the in-game save never reached the card"
    );
    assert_eq!(
        resume(d.path(), "Emerald"),
        None,
        "the autosave left a state to fall back to"
    );
    a.tick_ms(120_000);
    a.settle_saves();
    assert_eq!(resume(d.path(), "Emerald"), None);

    let plain = tmp_root_with_carts(&["Emerald"]);
    let mut a = playing(plain.path());
    a.tick_ms(60_000);
    a.settle_saves();
    assert!(resume(plain.path(), "Emerald").is_some());
}

#[test]
fn an_eject_suspends_for_one_insert() {
    let d = pure_root(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    eject_to_the_shelf(&mut a);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the suspend was spent on the shelf"
    );

    a.apply(Action::Insert);
    assert!(!a.starting_clean(), "a tap of A did not ask to resume");
    seat_again(&mut a);
    assert_eq!(resume(d.path(), "Emerald"), None);

    eject_to_the_shelf(&mut a);
    assert!(
        resume(d.path(), "Emerald").is_some(),
        "the second eject took no suspend"
    );
}

#[test]
fn holding_a_to_start_fresh_spends_the_suspend_as_well() {
    let d = pure_root(&["Emerald", "Fusion"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    eject_to_the_shelf(&mut a);
    assert!(resume(d.path(), "Emerald").is_some());

    a.apply(Action::GbaDown(Btn::A));
    frames(&mut a, 40);
    assert!(a.starting_clean(), "holding A did not start fresh");
    seat_again(&mut a);
    assert_eq!(
        resume(d.path(), "Emerald"),
        None,
        "a fresh start kept the old suspend in reserve"
    );
}

#[test]
fn auto_save_on_eject_switched_off_makes_an_eject_a_cold_removal() {
    let d = pure_root(&["Emerald", "Fusion"]);
    let (snapshot, _) = StubSnapshot::pair();
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            eject_save: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mut a = App::boot(d.path());
    a.set_snapshot(snapshot);
    a.on_core_ready();
    frames(&mut a, 120);
    eject_to_the_shelf(&mut a);
    assert_eq!(resume(d.path(), "Emerald"), None);
}

fn session_playing(root: &Path) -> Session {
    clocked(root);
    let mut s = Session::boot(root.to_path_buf());
    s.feed([RawEvent::Down(Btn::A)], 16);
    s.feed([RawEvent::Up(Btn::A)], 32);
    wait_playing(&mut s);
    s
}

fn wait_playing(s: &mut Session) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !matches!(s.app().phase(), Phase::Playing { .. }) {
        assert!(Instant::now() < deadline, "the cart never seated");
        s.update(FRAME);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn run(s: &mut Session, from: u64, frames: u64) -> u64 {
    let mut now = from;
    for _ in 0..frames {
        now += 16;
        s.feed([], now);
        s.update(FRAME);
        std::thread::sleep(Duration::from_millis(1));
    }
    now
}

fn pad(s: &Session) -> u16 {
    s.emu().expect("no core in the slot").input().0
}

fn overlays(s: &Session) -> Vec<Draw> {
    let mut out = Vec::new();
    s.app().draw(&mut out);
    out.retain(|d| !matches!(d, Draw::Game));
    out
}

#[test]
fn r2_never_fast_forwards_a_running_game() {
    let d = pure_root(&["Emerald", "Zzz"]);
    let mut s = session_playing(d.path());

    s.feed([RawEvent::Down(Btn::R2)], 1000);
    let mut now = run(&mut s, 1000, 40);
    assert_eq!(s.app().ff_badge(), None, "a held R2 raised the badge");
    assert_ne!(
        s.emu().map(EmuHandle::observed_speed),
        Some(Speed::Fast),
        "a held R2 ran the game fast"
    );
    s.feed([RawEvent::Up(Btn::R2)], now + 16);

    s.feed([RawEvent::Down(Btn::R2)], now + 100);
    s.feed([RawEvent::Up(Btn::R2)], now + 150);
    s.feed([RawEvent::Down(Btn::R2)], now + 250);
    s.feed([RawEvent::Up(Btn::R2)], now + 300);
    now = run(&mut s, now + 300, 40);
    assert_eq!(s.app().ff_badge(), None, "a double tap latched");
    assert_ne!(
        s.emu().map(EmuHandle::observed_speed),
        Some(Speed::Fast),
        "a double tap ran the game fast"
    );
    run(&mut s, now, 1);
}

#[test]
fn l2_never_rewinds_a_running_game() {
    let d = pure_root(&["Emerald", "Zzz"]);
    let mut s = session_playing(d.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut now = 1000;
    while !s.game_visible() {
        assert!(Instant::now() < deadline, "the game layer never came up");
        now = run(&mut s, now, 1);
    }
    let before = s
        .emu()
        .expect("no core")
        .request_state()
        .recv()
        .expect("no state");

    s.feed([RawEvent::Down(Btn::L2)], now + 16);
    now = run(&mut s, now + 16, 60);
    assert!(overlays(&s).is_empty(), "the rewind bar came up");
    let after = s
        .emu()
        .expect("no core")
        .request_state()
        .recv()
        .expect("no state");
    let frame = |state: Vec<u8>| u64::from_le_bytes(state.try_into().expect("a mock state"));
    assert!(
        frame(after) > frame(before),
        "the game ran backwards under a held L2"
    );
    s.feed([RawEvent::Up(Btn::L2)], now + 16);
}

#[test]
fn select_with_a_shoulder_reaches_the_game_and_saves_nothing() {
    let d = pure_root(&["Emerald", "Zzz"]);
    let mut s = session_playing(d.path());

    s.feed([RawEvent::Down(Btn::Select)], 1000);
    s.feed([RawEvent::Down(Btn::R1)], 1050);
    assert_ne!(pad(&s) & ButtonMask::R, 0, "R never reached the game");
    assert_ne!(
        pad(&s) & ButtonMask::SELECT,
        0,
        "SELECT never reached the game"
    );
    s.feed([RawEvent::Up(Btn::R1)], 1100);
    s.feed([RawEvent::Up(Btn::Select)], 1200);
    run(&mut s, 1200, 10);
    assert_eq!(pad(&s) & (ButtonMask::R | ButtonMask::SELECT), 0);

    let core = slot_store::core_for_platform(d.path(), "Emerald", Platform::Gba);
    assert!(
        StateRing::new(d.path(), Platform::Gba, core, "Emerald")
            .list()
            .unwrap()
            .is_empty(),
        "SELECT + R1 saved a state"
    );
    assert_eq!(s.app().toast(), None);
}

#[test]
fn x_and_y_are_not_turbo_buttons() {
    let d = pure_root(&["Emerald", "Zzz"]);
    let s = session_playing(d.path());
    assert!(!s.app().turbo());
}

#[test]
fn a_suspend_is_resumed_by_one_boot_and_gone_for_the_next() {
    const SUSPENDED_AT: u64 = 500_000;
    let d = pure_root(&["Emerald", "Zzz"]);
    write_slot_state(
        d.path(),
        &SlotState {
            cart: Some("Emerald".into()),
            clock_set: true,
            ..Default::default()
        },
    )
    .unwrap();
    let core = slot_store::core_for_platform(d.path(), "Emerald", Platform::Gba);
    let on_card = || persist::read_resume(d.path(), Platform::Gba, core, "Emerald");
    persist::flush(
        d.path(),
        Platform::Gba,
        core,
        "Emerald",
        Some(&SUSPENDED_AT.to_le_bytes()),
        None,
    )
    .unwrap();
    let frame_of = |s: &Session| {
        let state = s
            .emu()
            .expect("no core in the slot")
            .request_state()
            .recv()
            .expect("the core gave up no state");
        u64::from_le_bytes(state.try_into().expect("a mock state is eight bytes"))
    };

    let mut first = Session::boot(d.path().to_path_buf());
    wait_playing(&mut first);
    run(&mut first, 0, 5);
    assert!(
        frame_of(&first) >= SUSPENDED_AT,
        "the first boot did not pick the suspend up"
    );
    assert_eq!(
        on_card(),
        None,
        "the first boot left the suspend on the card"
    );
    drop(first);

    let mut second = Session::boot(d.path().to_path_buf());
    wait_playing(&mut second);
    run(&mut second, 0, 5);
    assert!(
        frame_of(&second) < SUSPENDED_AT,
        "the same suspend was loaded a second time"
    );
}
