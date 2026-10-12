mod common;

use common::{app_playing_with, tmp_root_with_carts, Loaded, StubSnapshot};
use slot::app::Phase;
use slot_input::{Action, Btn};

fn screen() -> Vec<u8> {
    let xrgb = vec![0x40u8; 240 * 160 * 4];
    slot::thumb::png(&xrgb).unwrap()
}

fn ejected(root: &std::path::Path) -> slot::app::App {
    let snap = StubSnapshot {
        state: vec![9u8; 1024],
        sav: None,
        thumb: Some(screen()),
        loaded: Loaded::default(),
    };
    let mut a = app_playing_with(root, "Emerald", Box::new(snap));
    a.apply(Action::Eject);
    for _ in 0..240 {
        a.update(1.0 / 60.0);
    }
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    a
}

#[test]
fn select_on_the_shelf_labels_the_cart_with_the_screen_it_left_on() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = ejected(d.path());
    assert_eq!(a.selected_stem(), Some("Emerald"));
    a.apply(Action::GbaDown(Btn::Select));
    let label = d.path().join("Labels/GBA/Emerald.png");
    let png = std::fs::read(&label).expect("no label written");
    let info = png::Decoder::new(png.as_slice()).read_info().unwrap();
    let (w, h) = (info.info().width, info.info().height);
    assert_eq!(h, 160, "the whole screen height is kept");
    assert!(w > 240, "the screen was not widened to the label: {w}x{h}");
    assert_eq!(a.toast(), Some(slot_ui::Toast::LabelSet));
}

#[test]
fn select_on_another_cart_labels_nothing() {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    let mut a = ejected(d.path());
    a.apply(Action::ShelfRight);
    for _ in 0..60 {
        a.update(1.0 / 60.0);
    }
    assert_ne!(a.selected_stem(), Some("Emerald"));
    a.apply(Action::GbaDown(Btn::Select));
    assert!(!d.path().join("Labels/GBA/Fusion.png").exists());
    assert!(!d.path().join("Labels/GBA/Emerald.png").exists());
}
