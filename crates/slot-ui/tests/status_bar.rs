use slot_ui::{rate_face, rate_text, status_icon_face, Draw, StatusBar, StatusIcon, TexId, OUT_W};

#[test]
fn every_status_glyph_is_in_the_symbols_font() {
    let faces: Vec<_> = StatusIcon::ALL
        .iter()
        .map(|i| status_icon_face(*i, true))
        .collect();
    for (icon, face) in StatusIcon::ALL.iter().zip(&faces) {
        assert!(
            face.rgba.chunks(4).any(|p| p[3] > 0),
            "{icon:?} drew nothing"
        );
    }
    for (i, a) in faces.iter().enumerate() {
        for b in &faces[i + 1..] {
            assert_ne!(a.rgba, b.rgba, "two status icons look the same");
        }
    }
}

#[test]
fn status_icons_share_one_box_and_dim_differs_from_lit() {
    let first = status_icon_face(StatusIcon::ALL[0], true);
    for icon in StatusIcon::ALL {
        let (lit, dim) = (status_icon_face(icon, true), status_icon_face(icon, false));
        assert_eq!((lit.w, lit.h), (first.w, first.h));
        assert_ne!(lit.rgba, dim.rgba);
    }
}

#[test]
fn rates_print_in_kilobytes_then_megabytes() {
    assert_eq!(rate_text(0), "0.0 KB/s");
    assert_eq!(rate_text(4_200), "4.2 KB/s");
    assert_eq!(rate_text(12_400), "12 KB/s");
    assert_eq!(rate_text(999_000), "999 KB/s");
    assert_eq!(rate_text(1_250_000), "1.2 MB/s");
    assert_eq!(rate_text(25_000_000), "25 MB/s");
}

#[test]
fn the_meter_keeps_one_width_whatever_it_reads() {
    let w = rate_face(0, 0).w;
    assert!(w > 0);
    assert_eq!(rate_face(999_000, 1_250_000).w, w);
    assert_ne!(rate_face(0, 0).rgba, rate_face(12_000, 0).rgba);
}

#[test]
fn wifi_sits_at_the_right_edge_with_bluetooth_then_the_meter_to_its_left() {
    let bar = StatusBar {
        bt: Some((TexId::from_raw(1), 20, 24)),
        wifi: Some((TexId::from_raw(2), 20, 24)),
        rate: Some((TexId::from_raw(3), 100, 24)),
    };
    let mut out = Vec::new();
    bar.draw(&mut out);
    let xs: Vec<(TexId, f32, f32)> = out
        .iter()
        .map(|d| match d {
            Draw::Tex { tex, x, w, .. } => (*tex, *x, *w),
            _ => panic!("only faces"),
        })
        .collect();
    let at = |t: u32| {
        xs.iter()
            .find(|(id, ..)| *id == TexId::from_raw(t as usize))
            .unwrap()
    };
    let (wifi, bt, rate) = (at(2), at(1), at(3));
    assert!(wifi.1 + wifi.2 <= OUT_W as f32);
    assert!(bt.1 + bt.2 <= wifi.1);
    assert!(rate.1 + rate.2 <= bt.1);
}

#[test]
fn the_shelf_radios_sit_left_of_the_clock_with_wifi_nearest() {
    let clock = slot_ui::Printed::new(TexId::from_raw(9), 60);
    let mut out = Vec::new();
    slot_ui::draw_footer_radios(
        clock,
        Some((TexId::from_raw(1), 20, 24)),
        Some((TexId::from_raw(2), 20, 24)),
        &mut out,
    );
    let x = |t: usize| {
        out.iter()
            .find_map(|d| match d {
                Draw::Tex { tex, x, w, .. } if *tex == TexId::from_raw(t) => Some((*x, *w)),
                _ => None,
            })
            .unwrap()
    };
    let (wifi, bt) = (x(2), x(1));
    assert!(wifi.0 + wifi.1 <= OUT_W as f32 - 60.0);
    assert!(bt.0 + bt.1 <= wifi.0);
}
