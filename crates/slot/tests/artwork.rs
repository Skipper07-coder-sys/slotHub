mod common;

use std::sync::{Arc, Mutex};

use common::{app_booting_at, tmp_root_with_carts, CLOCK_IS_SET};
use slot::app::App;
use slot::art::{ArtJobs, ArtState};
use slot::wifi::{WifiJob, WifiJobs, WifiState};
use slot_input::{Action, Btn};
use slot_store::{write_slot_state, SlotState};
use slot_ui::{QuickRow, WifiNetwork};
use tempfile::TempDir;

#[derive(Clone)]
struct FakeArt(Arc<Mutex<(ArtState, usize)>>);

impl ArtJobs for FakeArt {
    fn fetch(&mut self) {
        let mut s = self.0.lock().unwrap();
        *s = (ArtState::Fetching { done: 0, total: 0 }, s.1 + 1);
    }

    fn state(&self) -> ArtState {
        self.0.lock().unwrap().0
    }
}

#[derive(Clone)]
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

fn booted(wifi: WifiState) -> (TempDir, App, FakeArt) {
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
    let art = FakeArt(Arc::new(Mutex::new((ArtState::Idle, 0))));
    a.set_art_jobs(Box::new(art.clone()));
    a.set_wifi_jobs(Box::new(FixedWifi(wifi)));
    a.apply(Action::QuickMenu);
    for _ in 0..QuickRow::WifiArt.parent().unwrap().position() {
        press(&mut a, Btn::Down);
    }
    press(&mut a, Btn::A);
    for _ in 0..QuickRow::WifiArt.position() {
        press(&mut a, Btn::Down);
    }
    assert_eq!(a.quick_menu(), Some(QuickRow::WifiArt));
    (d, a, art)
}

fn press(a: &mut App, btn: Btn) {
    a.apply(Action::GbaDown(btn));
    a.apply(Action::GbaUp(btn));
}

fn text(a: &App) -> Option<String> {
    a.quick_live_text(QuickRow::WifiArt)
}

#[test]
fn the_row_counts_the_games_still_without_art() {
    let (_d, a, _) = booted(WifiState::Online);
    assert_eq!(text(&a).as_deref(), Some("Get 2"));
}

#[test]
fn without_wifi_nothing_is_fetched() {
    let (_d, mut a, art) = booted(WifiState::Off);
    assert_eq!(text(&a).as_deref(), Some("Needs Wi-Fi"));
    press(&mut a, Btn::A);
    assert_eq!(art.0.lock().unwrap().1, 0);
}

#[test]
fn fetched_art_lands_on_the_carts_when_the_run_ends() {
    let (d, mut a, art) = booted(WifiState::Online);
    press(&mut a, Btn::A);
    press(&mut a, Btn::A);
    assert_eq!(art.0.lock().unwrap().1, 1, "a second press while fetching");
    assert_eq!(text(&a).as_deref(), Some("Looking..."));
    let labels = d.path().join("Labels/GBA");
    std::fs::create_dir_all(&labels).unwrap();
    std::fs::write(labels.join("Emerald.png"), b"png").unwrap();
    art.0.lock().unwrap().0 = ArtState::Done {
        found: 1,
        missing: 1,
    };
    assert_eq!(text(&a).as_deref(), Some("Got 1, 1 Not Found"));
    a.tick_ms(1_000);
    a.tick_ms(2_000);
    let labelled: Vec<_> = a
        .carts()
        .filter(|c| c.label.is_some())
        .map(|c| c.stem.clone())
        .collect();
    assert_eq!(labelled, ["Emerald"]);
}
