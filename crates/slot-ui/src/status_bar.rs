use fontdue::Font;

use crate::draw::{Draw, TexId, OUT_W};
use crate::plate::{UndoFace, HINT_H};
use crate::power_menu::MENU_INK;
use crate::quick_menu::{DIM_INK, QUICK_EDGE};
use crate::text;

/// The radio glyphs the settings menu shows at its top right, the way muOS and
/// Knulli keep Bluetooth and the network in their status bar.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum StatusIcon {
    WifiOff,
    WifiSearching,
    WifiTrouble,
    Wifi1,
    Wifi2,
    Wifi3,
    Wifi4,
    BtOff,
    Bt,
    BtConnected,
}

impl StatusIcon {
    pub const ALL: [StatusIcon; 10] = [
        StatusIcon::WifiOff,
        StatusIcon::WifiSearching,
        StatusIcon::WifiTrouble,
        StatusIcon::Wifi1,
        StatusIcon::Wifi2,
        StatusIcon::Wifi3,
        StatusIcon::Wifi4,
        StatusIcon::BtOff,
        StatusIcon::Bt,
        StatusIcon::BtConnected,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn glyph(self) -> char {
        match self {
            StatusIcon::WifiOff => '\u{f092d}',
            StatusIcon::WifiSearching => '\u{f092f}',
            StatusIcon::WifiTrouble => '\u{f092b}',
            StatusIcon::Wifi1 => '\u{f091f}',
            StatusIcon::Wifi2 => '\u{f0922}',
            StatusIcon::Wifi3 => '\u{f0925}',
            StatusIcon::Wifi4 => '\u{f0928}',
            StatusIcon::BtOff => '\u{f00b2}',
            StatusIcon::Bt => '\u{f00af}',
            StatusIcon::BtConnected => '\u{f00b1}',
        }
    }

    /// The Wi-Fi glyph for a signal of 1 to 4 bars, full when the radio gives
    /// no level.
    pub fn wifi_bars(bars: Option<u8>) -> StatusIcon {
        match bars {
            Some(0 | 1) => StatusIcon::Wifi1,
            Some(2) => StatusIcon::Wifi2,
            Some(3) => StatusIcon::Wifi3,
            _ => StatusIcon::Wifi4,
        }
    }
}

const ICON_PX: f32 = 22.0;
const ARROW_PX: f32 = 14.0;
const RATE_PX: f32 = 15.0;
const ARROW_GAP: u32 = 2;
const HALF_GAP: u32 = 10;
const STATUS_TOP: f32 = 16.0;
const STATUS_GAP: f32 = 10.0;
const ARROW_DOWN: char = '\u{f0045}';
const ARROW_UP: char = '\u{f005d}';
/// The widest rate the meter prints, so the icons beside it never shift.
const RATE_WIDEST: &str = "999 KB/s";

/// One glyph, lit or dim, in a box every status icon shares.
pub fn status_icon_face(icon: StatusIcon, lit: bool) -> UndoFace {
    let Some(font) = crate::icon::symbols_font() else {
        return empty();
    };
    let ink = if lit { MENU_INK } else { DIM_INK };
    let (frame_w, frame_h, ascent) = frame(font);
    let mut rgba = vec![0u8; (frame_w * HINT_H * 4) as usize];
    let baseline = (HINT_H.saturating_sub(frame_h) / 2) as i32 + ascent;
    stamp_glyph(
        &mut rgba,
        frame_w,
        font,
        icon.glyph(),
        ICON_PX,
        0,
        baseline,
        ink,
    );
    UndoFace {
        rgba,
        w: frame_w,
        h: HINT_H,
    }
}

/// Bytes a second as the meter prints them: KB/s under a megabyte, else MB/s.
pub fn rate_text(bytes: u64) -> String {
    let kb = bytes as f64 / 1000.0;
    match kb {
        k if k < 10.0 => format!("{k:.1} KB/s"),
        k if k < 999.5 => format!("{k:.0} KB/s"),
        k if k < 9995.0 => format!("{:.1} MB/s", k / 1000.0),
        k => format!("{:.0} MB/s", k / 1000.0),
    }
}

/// "↓ 12 KB/s  ↑ 3.0 KB/s", each half a fixed width.
pub fn rate_face(down: u64, up: u64) -> UndoFace {
    let (Some(symbols), Some(label)) = (crate::icon::symbols_font(), text::label_font()) else {
        return empty();
    };
    let arrow_w = symbols.metrics(ARROW_DOWN, ARROW_PX).advance_width.ceil() as u32;
    let band_w = text_width(label, RATE_WIDEST);
    let half = arrow_w + ARROW_GAP + band_w;
    let w = 2 * half + HALF_GAP;
    let mut rgba = vec![0u8; (w * HINT_H * 4) as usize];
    let am = symbols.metrics(ARROW_DOWN, ARROW_PX);
    let arrow_baseline = (HINT_H as i32 - am.height as i32) / 2 + am.ymin + am.height as i32;
    for (i, (arrow, bytes)) in [(ARROW_DOWN, down), (ARROW_UP, up)].into_iter().enumerate() {
        let x = i as u32 * (half + HALF_GAP);
        stamp_glyph(
            &mut rgba,
            w,
            symbols,
            arrow,
            ARROW_PX,
            x as i32,
            arrow_baseline,
            MENU_INK,
        );
        let layout = text::Layout {
            lines: vec![rate_text(bytes).to_uppercase()],
            px: RATE_PX,
            tracking: 0.0,
        };
        let mut band = vec![0u8; (band_w * HINT_H * 4) as usize];
        text::draw_centred(&mut band, band_w, HINT_H, &layout, MENU_INK);
        blit(&mut rgba, w, &band, band_w, x + arrow_w + ARROW_GAP);
    }
    UndoFace { rgba, w, h: HINT_H }
}

/// The status corner of the settings menu: Bluetooth and Wi-Fi at the right
/// edge, the rate meter to their left while one is given.
pub struct StatusBar {
    pub bt: Option<(TexId, u32, u32)>,
    pub wifi: Option<(TexId, u32, u32)>,
    pub rate: Option<(TexId, u32, u32)>,
}

impl StatusBar {
    pub fn draw(&self, out: &mut Vec<Draw>) {
        let mut right = OUT_W as f32 - QUICK_EDGE;
        for (tex, w, h) in [self.wifi, self.bt, self.rate].into_iter().flatten() {
            right -= w as f32;
            out.push(Draw::Tex {
                x: right.round(),
                y: STATUS_TOP,
                w: w as f32,
                h: h as f32,
                tex,
                alpha: 1.0,
            });
            right -= STATUS_GAP;
        }
    }
}

fn empty() -> UndoFace {
    UndoFace {
        rgba: Vec::new(),
        w: 0,
        h: 0,
    }
}

fn text_width(font: &Font, s: &str) -> u32 {
    text::line_width(font, &s.to_uppercase(), RATE_PX, 0.0).ceil() as u32 + 2
}

/// The width, height and ascent of the box that holds every status glyph on
/// one baseline.
fn frame(font: &Font) -> (u32, u32, i32) {
    let (mut w, mut top, mut bottom) = (1i32, i32::MIN, i32::MAX);
    for icon in StatusIcon::ALL {
        let m = font.metrics(icon.glyph(), ICON_PX);
        w = w.max((m.xmin + m.width as i32).max(m.advance_width.ceil() as i32));
        top = top.max(m.ymin + m.height as i32);
        bottom = bottom.min(m.ymin);
    }
    (w as u32, (top - bottom).max(1) as u32, top)
}

#[allow(clippy::too_many_arguments)]
fn stamp_glyph(
    rgba: &mut [u8],
    w: u32,
    font: &Font,
    glyph: char,
    px: f32,
    x: i32,
    baseline: i32,
    ink: [u8; 3],
) {
    let h = (rgba.len() / 4) as u32 / w.max(1);
    let (m, cov) = font.rasterize(glyph, px);
    let x0 = x + m.xmin.max(0);
    let y0 = baseline - (m.ymin + m.height as i32);
    for gy in 0..m.height {
        for gx in 0..m.width {
            let (dx, dy) = (x0 + gx as i32, y0 + gy as i32);
            if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                continue;
            }
            let a = cov[gy * m.width + gx];
            if a == 0 {
                continue;
            }
            let at = ((dy as u32 * w + dx as u32) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[ink[0], ink[1], ink[2], a]);
        }
    }
}

fn blit(dst: &mut [u8], dst_w: u32, src: &[u8], src_w: u32, x: u32) {
    for y in 0..HINT_H {
        for sx in 0..src_w {
            let dx = x + sx;
            if dx >= dst_w {
                break;
            }
            let s = ((y * src_w + sx) * 4) as usize;
            if src[s + 3] == 0 {
                continue;
            }
            let d = ((y * dst_w + dx) * 4) as usize;
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
}
