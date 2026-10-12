//! A still frame written straight to the framebuffer, for the moments before
//! slot's own picture is up: `update.sh swap` runs `slot --splash "Installing
//! v1.7.3"` while a downloaded System goes in, and the frame stays on screen
//! until slot draws its first one. The boot logo is the backdrop, with the
//! message on a dark band below it.

use std::fs::OpenOptions;
use std::io::{self, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;

use slot_ui::text;

use crate::bootlogo::{self, Bmp};

const FB: &str = "/dev/fb0";
const FB_SYS: &str = "/sys/class/graphics/fb0";
const FBIOGET_VSCREENINFO: libc::c_ulong = 0x4600;

const INK: [u8; 3] = [255, 255, 255];
const BAND: [u8; 3] = [0, 0, 0];

/// How the framebuffer lays out a pixel, from FBIOGET_VSCREENINFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    pub width: usize,
    pub height: usize,
    /// Rows in the whole buffer; double-buffered panels have two pages.
    pub rows: usize,
    pub stride: usize,
    pub bytes: usize,
    /// (offset, length) of red, green, blue and alpha.
    pub channels: [(u32, u32); 4],
}

impl Format {
    fn read() -> Option<Format> {
        let fb = std::fs::File::open(FB).ok()?;
        let mut v = [0u32; 40];
        let rc = unsafe { libc::ioctl(fb.as_raw_fd(), FBIOGET_VSCREENINFO as _, v.as_mut_ptr()) };
        if rc != 0 {
            return None;
        }
        let (width, height, rows, bpp) = (v[0] as usize, v[1] as usize, v[3] as usize, v[6]);
        if width == 0 || height == 0 || !matches!(bpp, 16 | 24 | 32) {
            return None;
        }
        let bytes = bpp as usize / 8;
        let stride = std::fs::read_to_string(format!("{FB_SYS}/stride"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .filter(|s: &usize| *s >= v[2] as usize * bytes)
            .unwrap_or(v[2] as usize * bytes);
        Some(Format {
            width,
            height,
            rows: rows.max(height),
            stride,
            bytes,
            channels: [(v[8], v[9]), (v[11], v[12]), (v[14], v[15]), (v[17], v[18])],
        })
    }

    /// One page of `rgb` (width x height) in the framebuffer's own layout,
    /// alpha fully opaque so the display engine shows it.
    pub fn pack(&self, rgb: &[[u8; 3]]) -> Vec<u8> {
        let mut page = vec![0u8; self.stride * self.height];
        let [r, g, b, a] = self.channels;
        let put = |v: u8, (off, len): (u32, u32)| -> u32 {
            if len == 0 {
                0
            } else {
                ((v as u32) >> (8 - len.min(8))) << off
            }
        };
        let opaque = if a.1 == 0 {
            0
        } else {
            ((1u32 << a.1.min(8)) - 1) << a.0
        };
        for y in 0..self.height {
            for x in 0..self.width {
                let [pr, pg, pb] = rgb[y * self.width + x];
                let px = put(pr, r) | put(pg, g) | put(pb, b) | opaque;
                let at = y * self.stride + x * self.bytes;
                page[at..at + self.bytes].copy_from_slice(&px.to_le_bytes()[..self.bytes]);
            }
        }
        page
    }
}

/// The frame for a `width` x `height` panel: the logo centred on its own
/// backdrop colour, and `message` in a band across the lower part.
pub fn compose(logo: Option<&Bmp>, (width, height): (usize, usize), message: &str) -> Bmp {
    let portrait = height > width;
    let (w, h) = if portrait {
        (height, width)
    } else {
        (width, height)
    };
    let mut frame = logo
        .and_then(|l| bootlogo::compose(l, (w, h)))
        .unwrap_or(Bmp {
            width: w,
            height: h,
            rgb: vec![[0; 3]; w * h],
        });
    let band_h = h / 6;
    let band_y = h - band_h - h / 12;
    for px in &mut frame.rgb[band_y * w..(band_y + band_h) * w] {
        for (c, under) in px.iter_mut().enumerate() {
            *under = ((*under as u32 + BAND[c] as u32 * 3) / 4) as u8;
        }
    }
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, message, w as f32 * 0.9, 1, band_h as f32 * 0.45, 12.0);
        let cov = text::coverage(w as u32, band_h as u32, &layout);
        for (i, a) in cov.into_iter().enumerate() {
            if a == 0 {
                continue;
            }
            let px = &mut frame.rgb[band_y * w + i];
            for c in 0..3 {
                px[c] = ((INK[c] as u32 * a as u32 + px[c] as u32 * (255 - a as u32)) / 255) as u8;
            }
        }
    }
    if portrait {
        bootlogo::turned(&frame)
    } else {
        frame
    }
}

/// Writes the frame to every page of the framebuffer, so it shows whichever
/// page the panel is scanning out.
pub fn show(card: &Path, message: &str) -> io::Result<()> {
    let format = Format::read()
        .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "no usable /dev/fb0"))?;
    let logo = bootlogo::chosen(card).and_then(|b| bootlogo::decode(&b));
    let frame = compose(logo.as_ref(), (format.width, format.height), message);
    let page = format.pack(&frame.rgb);
    let mut fb = OpenOptions::new().write(true).open(FB)?;
    for n in 0..format.rows / format.height {
        fb.seek(SeekFrom::Start((n * format.stride * format.height) as u64))?;
        fb.write_all(&page)?;
    }
    fb.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argb(width: usize, height: usize) -> Format {
        Format {
            width,
            height,
            rows: height * 2,
            stride: width * 4,
            bytes: 4,
            channels: [(16, 8), (8, 8), (0, 8), (24, 8)],
        }
    }

    #[test]
    fn a_pixel_lands_in_the_framebuffers_own_order_and_opaque() {
        let page = argb(2, 1).pack(&[[0x11, 0x22, 0x33], [0, 0, 0]]);
        assert_eq!(&page[..4], &[0x33, 0x22, 0x11, 0xff], "BGRA in memory");
        assert_eq!(&page[4..], &[0, 0, 0, 0xff]);
        let rgb565 = Format {
            stride: 4,
            bytes: 2,
            channels: [(11, 5), (5, 6), (0, 5), (0, 0)],
            ..argb(2, 1)
        };
        assert_eq!(
            &rgb565.pack(&[[255, 255, 255], [0, 0, 0]])[..2],
            &[0xff, 0xff]
        );
    }

    #[test]
    fn the_message_is_drawn_in_the_band_and_the_logo_kept_above_it() {
        let logo = Bmp {
            width: 4,
            height: 4,
            rgb: vec![[200, 0, 0]; 16],
        };
        let f = compose(Some(&logo), (720, 480), "Installing v1.7.3");
        assert_eq!((f.width, f.height), (720, 480));
        assert_eq!(f.rgb[240 * 720 + 360], [200, 0, 0], "logo centre");
        let band = &f.rgb[(480 - 80 - 40) * 720..(480 - 40) * 720];
        assert!(band.iter().any(|p| p[1] > 200), "no white text in the band");
        assert!(band.contains(&[50, 0, 0]), "band not darkened");
    }

    #[test]
    fn a_portrait_panel_gets_the_frame_turned() {
        let f = compose(None, (480, 640), "Installing");
        assert_eq!((f.width, f.height), (480, 640));
    }
}
