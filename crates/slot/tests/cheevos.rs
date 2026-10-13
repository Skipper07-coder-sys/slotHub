mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::{App, Phase};
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_cheevos::{Cheevos, Event, Region, Secret};
use slot_input::{Action, Btn};
use slot_store::{read_cheevos, write_cheevos, write_slot_state, SlotState, CHEEVOS_FILE};
use slot_ui::{CheevosSetup, QuickRow, WifiNetwork, CHEEVO_SHOW_MS};
use tempfile::TempDir;

#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    events: Mutex<Vec<Event>>,
}

impl Fake {
    fn said(&self, event: Event) {
        self.events.lock().unwrap().push(event);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl Cheevos for Fake {
    fn sign_in(&self, username: &str, password: Secret) {
        let call = format!("sign_in {username} {}", password.expose());
        self.calls.lock().unwrap().push(call);
    }
    fn resume(&self, username: &str, token: &Secret) {
        let call = format!("resume {username} {}", token.expose());
        self.calls.lock().unwrap().push(call);
    }
    fn sign_out(&self) {
        self.calls.lock().unwrap().push("sign_out".into());
    }
    fn signed_in(&self) -> bool {
        false
    }
    fn load_game(&self, _md5: &str) {}
    fn unload_game(&self) {}
    fn set_memory(&self, _regions: Vec<Region>) {}
    fn do_frame(&self) {}
    fn idle(&self) {}
    fn reset_progress(&self) {}
    fn take_events(&self) -> Vec<Event> {
        std::mem::take(&mut self.events.lock().unwrap())
    }
}

struct FixedWifi(WifiState);

impl WifiJobs for FixedWifi {
    fn ask(&mut self, _job: WifiJob) {}

    fn state(&self) -> WifiState {
        self.0
    }

    fn scan(&mut self) {}

    fn scanned(&self) -> Option<Vec<WifiNetwork>> {
        Some(Vec::new())
    }
}

fn booted(saved: Option<(&str, &str)>) -> (TempDir, App, Arc<Fake>) {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_slot_state(
        d.path(),
        &SlotState {
            clock_set: true,
            wifi: true,
            ..SlotState::default()
        },
    )
    .expect("write slot.state");
    if let Some((name, token)) = saved {
        write_cheevos(d.path(), name, token).unwrap();
    }
    let (mut a, _) = app_booting_at(d.path(), CLOCK_IS_SET);
    let fake = Arc::new(Fake::default());
    a.set_cheevos(fake.clone());
    a.set_wifi_jobs(Box::new(FixedWifi(WifiState::Online)));
    (d, a, fake)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn open_row(a: &mut App) {
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::Wifi.position() {
        press(a, Btn::Down);
    }
    press(a, Btn::A);
    for _ in 0..QuickRow::Cheevos.position() {
        press(a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::Cheevos));
}

fn type_keys(a: &mut App, n: usize) {
    // q w e r ... along the second row of the first page
    press(a, Btn::Down);
    for _ in 0..n {
        press(a, Btn::A);
        press(a, Btn::Right);
    }
}

fn tick(a: &mut App, at: u64) {
    a.tick_ms(at);
}

#[test]
fn the_row_sits_last_on_the_wifi_page_and_says_off_when_signed_out() {
    let (_d, mut a, _fake) = booted(None);
    open_row(&mut a);
    assert_eq!(QuickRow::Cheevos.label(), "RetroAchievements");
    assert_eq!(QuickRow::Cheevos.parent(), Some(QuickRow::Wifi));
    assert_eq!(a.quick_live_text(QuickRow::Cheevos).as_deref(), Some("Off"));
    assert!(a.cheevos_for_game().is_none());
}

#[test]
fn signing_in_asks_a_name_then_a_password_and_keeps_only_the_token() {
    let (d, mut a, fake) = booted(None);
    open_row(&mut a);
    press(&mut a, Btn::A);
    assert!(matches!(a.phase(), Phase::CheevosSetup { .. }));
    type_keys(&mut a, 2);
    press(&mut a, Btn::Start);
    type_keys(&mut a, 3);
    press(&mut a, Btn::Start);
    assert_eq!(fake.calls(), ["sign_in qw qwe"]);
    assert_eq!(
        a.cheevos_setup(),
        Some(&CheevosSetup::SigningIn {
            username: "qw".into()
        })
    );
    assert_eq!(a.cheevos_text(), "Signing In...");

    fake.said(Event::SignedIn {
        username: "qw".into(),
        token: Secret::new("tok123".into()),
    });
    tick(&mut a, 1000);
    assert_eq!(
        a.cheevos_setup(),
        Some(&CheevosSetup::SignedIn {
            username: "qw".into()
        })
    );
    assert_eq!(read_cheevos(d.path()), Some(("qw".into(), "tok123".into())));
    let conf = std::fs::read_to_string(d.path().join(CHEEVOS_FILE)).unwrap();
    assert!(
        !conf.contains("qwe\n"),
        "the password reached the card: {conf}"
    );
    assert!(a.cheevos_for_game().is_some());

    press(&mut a, Btn::A);
    assert_eq!(a.quick_menu(), Some(QuickRow::Cheevos));
    assert_eq!(a.cheevos_text(), "qw");

    // A again signs out and forgets the card's token.
    press(&mut a, Btn::A);
    assert_eq!(a.cheevos_text(), "Off");
    assert_eq!(read_cheevos(d.path()), None);
    assert_eq!(fake.calls().last().map(String::as_str), Some("sign_out"));
    assert!(a.cheevos_for_game().is_none());
}

#[test]
fn a_refused_password_says_so_and_offers_another_go() {
    let (_d, mut a, fake) = booted(None);
    open_row(&mut a);
    press(&mut a, Btn::A);
    type_keys(&mut a, 1);
    press(&mut a, Btn::Start);
    type_keys(&mut a, 1);
    press(&mut a, Btn::Start);
    fake.said(Event::SignInFailed {
        message: "Invalid".into(),
        rejected: true,
    });
    tick(&mut a, 1000);
    assert!(matches!(
        a.cheevos_setup(),
        Some(CheevosSetup::Failed { message, .. }) if message == "Wrong name or password"
    ));
    assert_eq!(a.cheevos_text(), "Failed");
    press(&mut a, Btn::A);
    assert!(matches!(
        a.cheevos_setup(),
        Some(CheevosSetup::Password { .. })
    ));
    a.apply(Action::QuickMenu);
    assert_eq!(a.quick_menu(), Some(QuickRow::Cheevos));
}

#[test]
fn a_saved_sign_in_is_resumed_with_its_token_once_wifi_is_up() {
    let (_d, mut a, fake) = booted(Some(("me", "tok")));
    assert_eq!(a.cheevos_text(), "me");
    tick(&mut a, 1000);
    assert_eq!(fake.calls(), ["resume me tok"]);
    assert!(
        a.cheevos_for_game().is_some(),
        "a game started before the server answers waits for the sign-in"
    );
    fake.said(Event::SignedIn {
        username: "me".into(),
        token: Secret::new("tok".into()),
    });
    tick(&mut a, 2000);
    assert!(a.cheevos_for_game().is_some());
}

#[test]
fn a_token_the_server_refuses_is_forgotten() {
    let (d, mut a, fake) = booted(Some(("me", "tok")));
    tick(&mut a, 1000);
    fake.said(Event::SignInFailed {
        message: String::new(),
        rejected: true,
    });
    tick(&mut a, 2000);
    assert_eq!(read_cheevos(d.path()), None);
    assert_eq!(a.cheevos_text(), "Failed");
    assert!(a.cheevos_for_game().is_none());
}

#[test]
fn unlocks_pop_up_one_after_another() {
    let (_d, mut a, fake) = booted(None);
    fake.said(Event::Unlocked {
        title: "First Steps".into(),
        description: "Leave the house".into(),
        points: 5,
    });
    fake.said(Event::Unlocked {
        title: "Second".into(),
        description: String::new(),
        points: 10,
    });
    tick(&mut a, 1000);
    assert_eq!(
        a.cheevo_shown().map(|c| c.title.as_str()),
        Some("First Steps")
    );
    tick(&mut a, 1000 + CHEEVO_SHOW_MS);
    assert_eq!(a.cheevo_shown().map(|c| c.title.as_str()), Some("Second"));
    tick(&mut a, 1000 + 2 * CHEEVO_SHOW_MS);
    assert_eq!(a.cheevo_shown(), None);
}

#[test]
fn every_platform_hashes_and_maps_as_its_own_rcheevos_console() {
    use slot::cheevos::{ra_console, rom_hash};
    use slot_retro::RaConsole;
    use slot_store::Platform;
    assert_eq!(ra_console(Platform::Gba), RaConsole::Gba);
    assert_eq!(ra_console(Platform::Gb), RaConsole::Gb);
    assert_eq!(ra_console(Platform::Gbc), RaConsole::Gbc);
    let d = tempfile::tempdir().unwrap();
    let rom = d.path().join("game.gb");
    std::fs::write(&rom, b"abc").unwrap();
    for platform in Platform::ALL {
        let console = ra_console(platform);
        assert_eq!(
            slot_cheevos::hash_method(console.rc_id()),
            Some(slot_cheevos::HashMethod::WholeFile),
            "{platform:?}"
        );
        // rc_hash for these is the MD5 of the whole file.
        assert_eq!(
            rom_hash(console, &rom).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
    }
}
