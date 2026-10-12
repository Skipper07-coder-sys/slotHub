use slot_retro::{GBA_H, GBA_W};

pub fn png(xrgb8888: &[u8]) -> Option<Vec<u8>> {
    let n = (GBA_W * GBA_H) as usize;
    if xrgb8888.len() < n * 4 {
        return None;
    }
    let mut rgb = Vec::with_capacity(n * 3);
    for px in xrgb8888[..n * 4].chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }

    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, GBA_W, GBA_H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(&rgb).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// A screen `png` made as wide as a `lw` x `lh` label, so the label shows
/// all of it instead of cropping top and bottom: each row runs on at both
/// sides in its own edge colour.
pub fn widened(png: &[u8], (lw, lh): (u32, u32)) -> Option<Vec<u8>> {
    let mut dec = png::Decoder::new(png);
    dec.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let bytes = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return None,
    };
    let (w, h) = (info.width as usize, info.height as usize);
    if lh == 0 || w == 0 || h == 0 {
        return None;
    }
    let want = (h * lw as usize).div_ceil(lh as usize);
    if want <= w {
        return Some(png.to_vec());
    }
    let left = (want - w) / 2;
    let mut rgb = Vec::with_capacity(want * h * 3);
    for row in buf[..w * h * bytes].chunks_exact(w * bytes) {
        let px = |x: usize| &row[x * bytes..x * bytes + 3];
        for x in 0..want {
            rgb.extend_from_slice(px(x.saturating_sub(left).min(w - 1)));
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, want as u32, h as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().ok()?;
    writer.write_image_data(&rgb).ok()?;
    writer.finish().ok()?;
    Some(out)
}
