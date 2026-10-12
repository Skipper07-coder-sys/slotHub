use std::path::Path;

fn png(path: &Path, w: u32, h: u32, rgba: &[u8]) {
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), w, h);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn px(out: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    out[((y * w + x) * 4) as usize..][..4].try_into().unwrap()
}

#[test]
fn a_logo_is_fitted_whole_on_a_colour_of_its_own() {
    // A 4x2 logo: a yellow middle with see-through ends.
    let clear = [0, 0, 0, 0];
    let yellow = [255, 220, 0, 255];
    let rgba = [clear, yellow, yellow, clear, clear, yellow, yellow, clear].concat();
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("logo.png");
    png(&path, 4, 2, &rgba);

    let (w, h) = (100, 50);
    let out = slot_ui::fit_label(&path, w, h).unwrap();
    let corner = px(&out, w, 0, 0);
    assert_eq!(corner[3], 255, "label is opaque");
    assert_eq!(
        &corner[..3],
        &[63, 55, 0],
        "a dark backdrop under a light logo"
    );
    assert_eq!(
        px(&out, w, 50, 25),
        [255, 220, 0, 255],
        "logo in the middle"
    );
    assert_eq!(
        px(&out, w, 50, 2),
        corner,
        "logo fitted inside, not cropped to fill"
    );
}

#[test]
fn a_picture_still_fills_the_label() {
    let blue = [0, 0, 200, 255];
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("box.png");
    png(&path, 2, 2, &[blue; 4].concat());
    let out = slot_ui::fit_label(&path, 10, 5).unwrap();
    assert!(out.chunks_exact(4).all(|p| p == blue));
}
