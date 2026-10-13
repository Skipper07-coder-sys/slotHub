use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_ui::{
    quick_caret_face, quick_label_face, quick_legend_faces, quick_value_face, rate_face,
    status_icon_face, QuickMenu, QuickMenuFaces, QuickRow, QuickValue, StatusBar, StatusIcon,
    UndoFace,
};

/// Draws the settings menu with its status corner to $SCRATCH_PNG.
#[test]
fn render_status_bar() {
    let Ok(out) = std::env::var("SCRATCH_PNG") else {
        return;
    };
    let mut faces: Vec<UndoFace> = Vec::new();
    let mut add = |f: UndoFace| {
        let size = (f.w, f.h);
        faces.push(f);
        (TexId::from_raw(faces.len() - 1), size.0, size.1)
    };
    let labels = QuickRow::ALL
        .iter()
        .map(|r| add(quick_label_face(*r)))
        .collect();
    let values = QuickValue::ALL
        .iter()
        .map(|v| [false, true].map(|lit| add(quick_value_face(v.text(), lit))))
        .collect();
    let carets = [false, true].map(|r| add(quick_caret_face(r)));
    let legend = quick_legend_faces().map(|f| {
        let (t, w, _) = add(f);
        (t, w)
    });
    let bt = add(status_icon_face(StatusIcon::BtConnected, true));
    let wifi = add(status_icon_face(StatusIcon::Wifi3, true));
    let rate = add(rate_face(48_300, 2_100));
    let mut live = [None; QuickRow::ALL.len()];
    for (row, text) in [
        (QuickRow::Wifi, "Home Net"),
        (QuickRow::Bluetooth, "My Headphones"),
    ] {
        live[row.index()] = Some([false, true].map(|lit| add(quick_value_face(text, lit))));
    }
    let menu_faces = QuickMenuFaces {
        labels,
        values,
        carets,
        legend,
    };
    let mut values = [None; QuickRow::ALL.len()];
    values[QuickRow::WifiPower.index()] = Some(QuickValue::On);
    let mut list = Vec::new();
    QuickMenu {
        row: QuickRow::Wifi,
        pure: false,
        values,
        live: &live,
        faces: Some(&menu_faces),
        ticker: None,
    }
    .draw(&mut list);
    StatusBar {
        bt: Some(bt),
        wifi: Some(wifi),
        rate: Some(rate),
    }
    .draw(&mut list);

    write_png(&out, &list, &faces, [0, 0, 0]);
}

/// Draws the shelf's bottom strip, charging, to $SCRATCH_FOOTER_PNG.
#[test]
fn render_shelf_footer() {
    let Ok(out) = std::env::var("SCRATCH_FOOTER_PNG") else {
        return;
    };
    let mut faces: Vec<UndoFace> = Vec::new();
    let mut add = |f: UndoFace| {
        let size = (f.w, f.h);
        faces.push(f);
        (TexId::from_raw(faces.len() - 1), size.0, size.1)
    };
    let bolt = slot_ui::icon_face(slot_ui::Icon::Charging, slot_ui::BOLT_PX, slot_ui::HUD_INK);
    let (bolt, _, _) = add(UndoFace {
        rgba: bolt.rgba,
        w: bolt.w,
        h: bolt.h,
    });
    let (pt, pw, _) = add(slot_ui::word_face("68%"));
    let (ct, cw, _) = add(slot_ui::word_face("12:51"));
    let bt = add(status_icon_face(StatusIcon::BtConnected, true));
    let wifi = add(status_icon_face(StatusIcon::Wifi3, true));
    let battery = Some(slot_power::Battery {
        percent: 68,
        charge: slot_power::Charge::Charging,
    });
    let percent = slot_ui::Printed::new(pt, pw);
    let mut list = Vec::new();
    slot_ui::draw_footer(
        battery,
        percent,
        Some(bolt),
        slot_ui::Printed::new(ct, cw),
        &mut list,
    );
    slot_ui::draw_footer_radios(battery, percent, Some(bt), Some(wifi), &mut list);
    write_png(&out, &list, &faces, [0x50, 0x48, 0x40]);
}

fn write_png(out: &str, list: &[Draw], faces: &[UndoFace], bg: [u8; 3]) {
    let (w, h) = (OUT_W as usize, OUT_H as usize);
    let mut px = vec![0u8; w * h * 4];
    for p in px.chunks_mut(4) {
        p.copy_from_slice(&[bg[0], bg[1], bg[2], 255]);
    }
    let blend = |dst: &mut [u8], c: [f32; 3], a: f32| {
        for k in 0..3 {
            dst[k] = (c[k] * 255.0 * a + dst[k] as f32 * (1.0 - a)) as u8;
        }
    };
    for d in list {
        match *d {
            Draw::Rect {
                x,
                y,
                w: rw,
                h: rh,
                colour,
            } => {
                for yy in y as usize..((y + rh) as usize).min(h) {
                    for xx in x as usize..((x + rw) as usize).min(w) {
                        let at = (yy * w + xx) * 4;
                        blend(
                            &mut px[at..at + 4],
                            [colour[0], colour[1], colour[2]],
                            colour[3],
                        );
                    }
                }
            }
            Draw::Tex {
                x,
                y,
                w: tw,
                h: th,
                tex,
                alpha,
            } => {
                let Some(f) = faces
                    .iter()
                    .enumerate()
                    .find_map(|(i, f)| (TexId::from_raw(i) == tex).then_some(f))
                else {
                    continue;
                };
                for row in 0..th as usize {
                    for col in 0..tw as usize {
                        let (sx, sy) = (
                            col * f.w as usize / tw as usize,
                            row * f.h as usize / th as usize,
                        );
                        let s = (sy * f.w as usize + sx) * 4;
                        let a = f.rgba[s + 3] as f32 / 255.0 * alpha;
                        let (dx, dy) = (x as usize + col, y as usize + row);
                        if a > 0.0 && dx < w && dy < h {
                            let at = (dy * w + dx) * 4;
                            let c = [
                                f.rgba[s] as f32 / 255.0,
                                f.rgba[s + 1] as f32 / 255.0,
                                f.rgba[s + 2] as f32 / 255.0,
                            ];
                            blend(&mut px[at..at + 4], c, a);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let f = std::fs::File::create(out).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
}
