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
    let menu_faces = QuickMenuFaces {
        labels,
        values,
        carets,
        legend,
    };
    let live = [None; QuickRow::ALL.len()];
    let mut values = [None; QuickRow::ALL.len()];
    values[QuickRow::WifiPower.index()] = Some(QuickValue::On);
    let mut list = Vec::new();
    QuickMenu {
        row: QuickRow::Wifi,
        pure: false,
        values,
        live: &live,
        faces: Some(&menu_faces),
    }
    .draw(&mut list);
    StatusBar {
        bt: Some(bt),
        wifi: Some(wifi),
        rate: Some(rate),
    }
    .draw(&mut list);

    let (w, h) = (OUT_W as usize, OUT_H as usize);
    let mut px = vec![0u8; w * h * 4];
    for p in px.chunks_mut(4) {
        p.copy_from_slice(&[0, 0, 0, 255]);
    }
    let blend = |dst: &mut [u8], c: [f32; 3], a: f32| {
        for k in 0..3 {
            dst[k] = (c[k] * 255.0 * a + dst[k] as f32 * (1.0 - a)) as u8;
        }
    };
    for d in &list {
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
                x, y, tex, alpha, ..
            } => {
                let Some(f) = faces
                    .iter()
                    .enumerate()
                    .find_map(|(i, f)| (TexId::from_raw(i) == tex).then_some(f))
                else {
                    continue;
                };
                for row in 0..f.h as usize {
                    for col in 0..f.w as usize {
                        let s = (row * f.w as usize + col) * 4;
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
    let f = std::fs::File::create(&out).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&px).unwrap();
}
