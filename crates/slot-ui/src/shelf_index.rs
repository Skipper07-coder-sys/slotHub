use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use slot_gfx::{Draw, TexId, OUT_W};

use crate::cart::CART_H;
use crate::plate::{UndoFace, HINT_H};
use crate::shelf::{rest_y, OMEGA};
use crate::slot_chrome::{ease, slot_name_y};
use crate::text;

/// Inter SemiBold, cut down to # A-Z and the accented capitals.
const RAIL_OTF: &[u8] = include_bytes!("../assets/inter-semibold-rail.otf");

pub const RAIL_PX: f32 = 26.0;
const RAIL_GAP: f32 = 40.0;
const RAIL_SHRINK_PX: f32 = 4.0;
const RAIL_INK: [u8; 3] = [0xf6, 0xf4, 0xef];
const RAIL_ALPHA: f32 = 0.85;
const RAIL_FADE: f32 = 0.40;
const RAIL_REACH: f32 = 2.6;
const SPRING_STEP: f32 = 1.0 / 240.0;
const MAX_SPRING_DT: f32 = 0.25;

pub const TITLE_WIN_W: u32 = 184;
const TITLE_PX: f32 = 15.0;
const TITLE_INK: [u8; 3] = [0x70, 0x70, 0x7e];
const TITLE_LIGHT: [u8; 3] = [0x96, 0x96, 0xa5];
const TITLE_LIGHT_ALPHA: u32 = 170;
const TITLE_SHADE: [u8; 3] = [0x00, 0x00, 0x00];
const TITLE_SINK: f32 = 14.0;

const MARQUEE_GAP: u32 = 60;
const MARQUEE_PX_PER_S: f32 = 32.0;
pub const MARQUEE_WAIT_MS: u64 = 1000;
const MARQUEE_REST_MS: u64 = 800;

pub const INDEX_IN_MS: u64 = 150;
pub const INDEX_HOLD_MS: u64 = 2500;
pub const INDEX_OUT_MS: u64 = 800;

fn rail_font() -> Option<&'static Font> {
    static FONT: OnceLock<Option<Font>> = OnceLock::new();
    FONT.get_or_init(|| Font::from_bytes(RAIL_OTF, FontSettings::default()).ok())
        .as_ref()
}

/// How visible the letter rail and the slot title are. `shown` is how long
/// ago they started showing, `moved` how long ago the shelf last moved, and
/// `hold` how long they stay after that move. Fast scrolling keeps moving, so
/// they stay up; they come in once, not again on every cart.
pub fn index_alpha(shown: u64, moved: u64, hold: u64) -> f32 {
    let fade_in = ease((shown as f32 / INDEX_IN_MS as f32).min(1.0));
    if moved < hold {
        return fade_in;
    }
    fade_in * (1.0 - ease(((moved - hold) as f32 / INDEX_OUT_MS as f32).min(1.0)))
}

/// How long the index stays after the shelf stops on a name `strip_w` wide:
/// the usual hold, or long enough for a scrolling name to come all the way
/// round once.
pub fn index_hold(strip_w: u32) -> u64 {
    if strip_w <= TITLE_WIN_W {
        return INDEX_HOLD_MS;
    }
    let lap = (strip_w + MARQUEE_GAP) as f32 / MARQUEE_PX_PER_S * 1000.0;
    INDEX_HOLD_MS.max(MARQUEE_WAIT_MS + lap.ceil() as u64 + MARQUEE_REST_MS)
}

/// The distinct first letters of a shelf, in shelf order.
pub fn shelf_letters<'a>(stems: impl IntoIterator<Item = &'a str>, out: &mut Vec<char>) {
    out.clear();
    for stem in stems {
        let c = slot_store::initial(stem);
        if !out.contains(&c) {
            out.push(c);
        }
    }
}

/// One letter of the rail, white on clear, drawn at the rail's full size and
/// scaled down for the letters off to the sides.
pub fn rail_glyph_face(ch: char) -> UndoFace {
    let thin = rail_font().filter(|f| f.lookup_glyph_index(ch) != 0);
    let Some(font) = thin.or_else(text::label_font) else {
        return UndoFace {
            rgba: vec![0; 4],
            w: 1,
            h: 1,
        };
    };
    let (m, cov) = font.rasterize(ch, RAIL_PX);
    let h = RAIL_PX.ceil() as u32 + 4;
    let w = (m.advance_width.ceil() as u32).max(m.width as u32).max(1) + 4;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let cap = font.metrics('H', RAIL_PX).height as f32;
    let baseline = ((h as f32 + cap) / 2.0).round() as i32;
    let x0 = (w as i32 - m.width as i32) / 2;
    let y0 = baseline - (m.height as i32 + m.ymin);
    for gy in 0..m.height {
        for gx in 0..m.width {
            let (dx, dy) = (x0 + gx as i32, y0 + gy as i32);
            if dx < 0 || dy < 0 || dx >= w as i32 || dy >= h as i32 {
                continue;
            }
            let a = cov[gy * m.width + gx];
            let at = ((dy as u32 * w + dx as u32) * 4) as usize;
            rgba[at..at + 4].copy_from_slice(&[RAIL_INK[0], RAIL_INK[1], RAIL_INK[2], a]);
        }
    }
    UndoFace { rgba, w, h }
}

pub fn rail_y() -> f32 {
    rest_y(CART_H as f32) / 2.0
}

/// The row of first letters over the shelf. It rides a spring like the carts
/// do, so a letter jump slides the row instead of swapping it.
#[derive(Default)]
pub struct LetterRail {
    pub letters: Vec<char>,
    pos: f32,
    vel: f32,
    current: Option<char>,
}

impl LetterRail {
    pub fn pos(&self) -> f32 {
        self.pos
    }

    /// Takes the shelf's letters and the selected cart's letter. A new set of
    /// letters (another shelf, a rescan) puts the rail straight on its letter.
    pub fn update(&mut self, letters: &[char], current: Option<char>, dt: f32) {
        let Some(at) = current.and_then(|c| letters.iter().position(|l| *l == c)) else {
            self.letters.clear();
            self.current = None;
            return;
        };
        let target_at = at as f32;
        if self.letters != letters {
            self.letters.clear();
            self.letters.extend_from_slice(letters);
            self.pos = target_at;
            self.vel = 0.0;
        }
        self.current = current;
        let n = self.letters.len() as f32;
        let target = self.pos + (target_at - self.pos + n / 2.0).rem_euclid(n) - n / 2.0;
        // A long frame (a hitch, a resume) lands on the letter rather than
        // throwing the spring.
        if dt > MAX_SPRING_DT {
            self.pos = target_at;
            self.vel = 0.0;
            return;
        }
        let steps = (dt / SPRING_STEP).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            let accel = -2.0 * OMEGA * self.vel - OMEGA * OMEGA * (self.pos - target);
            self.vel += accel * h;
            self.pos += self.vel * h;
        }
        if self.vel.abs() < 1e-3 && (self.pos - target).abs() < 1e-3 {
            self.pos = target_at;
            self.vel = 0.0;
        }
    }

    pub fn draw(
        &self,
        alpha: f32,
        face: impl Fn(char) -> Option<(TexId, u32, u32)>,
        out: &mut Vec<Draw>,
    ) {
        let n = self.letters.len() as i32;
        if n == 0 || alpha <= 0.0 {
            return;
        }
        // Each side shows at most the letters that are not already on the
        // other side, so a short shelf never shows a letter twice.
        let side = ((n - 1) / 2).min(2) as f32 + 0.6;
        let reach = RAIL_REACH.min(side);
        let base = self.pos.round() as i32;
        for k in base - 3..=base + 3 {
            let off = k as f32 - self.pos;
            let a = off.abs();
            if a > reach {
                continue;
            }
            let ch = self.letters[k.rem_euclid(n) as usize];
            let Some((tex, w, h)) = face(ch) else {
                continue;
            };
            let scale = (RAIL_PX - RAIL_SHRINK_PX * a.min(2.5)) / RAIL_PX;
            let fade = (1.0 - RAIL_FADE * a).max(0.0) * ((reach - a) / 0.6).min(1.0);
            let (w, h) = (w as f32 * scale, h as f32 * scale);
            out.push(Draw::Tex {
                x: OUT_W as f32 / 2.0 + off * RAIL_GAP - w / 2.0,
                y: rail_y() - h / 2.0,
                w,
                h,
                tex,
                alpha: RAIL_ALPHA * fade * alpha,
            });
        }
    }
}

/// The selected cart's name as it is pressed into the slot: plastic grey,
/// shaded above and lit below. The strip is as wide as the name needs; a name
/// wider than the slot scrolls through [`title_window`].
pub fn title_strip(name: &str) -> UndoFace {
    let h = HINT_H;
    let Some(font) = text::label_font() else {
        return UndoFace {
            rgba: vec![0; (TITLE_WIN_W * h * 4) as usize],
            w: TITLE_WIN_W,
            h,
        };
    };
    let layout = text::fit(font, name, f32::INFINITY, 1, TITLE_PX, TITLE_PX);
    let ink = layout.lines.first().map_or(0.0, |l| {
        text::line_width(font, l, layout.px, layout.tracking)
    });
    let w = (ink.ceil() as u32 + 2).max(TITLE_WIN_W);
    let cov = text::coverage(w, h, &layout);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let mut layer = |dy: i32, colour: [u8; 3], strength: u32| {
        for y in 0..h as i32 {
            let from = y - dy;
            if from < 0 || from >= h as i32 {
                continue;
            }
            for x in 0..w as usize {
                let a = cov[from as usize * w as usize + x] as u32 * strength / 255;
                if a > 0 {
                    let at = (y as usize * w as usize + x) * 4;
                    text::blend(&mut rgba[at..at + 4], a, colour);
                }
            }
        }
    };
    layer(1, TITLE_LIGHT, TITLE_LIGHT_ALPHA);
    layer(-1, TITLE_SHADE, 255);
    layer(0, TITLE_INK, 255);
    UndoFace { rgba, w, h }
}

pub fn title_scrolls(strip: &UndoFace) -> bool {
    strip.w > TITLE_WIN_W
}

/// How far a scrolling name has moved, `since` ms after it was picked. It
/// waits a beat so the start can be read, then loops with a gap.
pub fn marquee_shift(strip_w: u32, since: u64) -> u32 {
    if strip_w <= TITLE_WIN_W || since < MARQUEE_WAIT_MS {
        return 0;
    }
    let run = (since - MARQUEE_WAIT_MS) as f32 / 1000.0;
    (run * MARQUEE_PX_PER_S) as u32 % (strip_w + MARQUEE_GAP)
}

/// The slot-wide piece of a scrolling name starting `shift` px in, with the
/// name coming round again after the gap.
pub fn title_window(strip: &UndoFace, shift: u32) -> UndoFace {
    let (w, h) = (TITLE_WIN_W, strip.h);
    if strip.w <= w {
        return UndoFace {
            rgba: strip.rgba.clone(),
            w: strip.w,
            h,
        };
    }
    let lap = strip.w + MARQUEE_GAP;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for x in 0..w {
        let sx = (x + shift) % lap;
        if sx >= strip.w {
            continue;
        }
        for y in 0..h {
            let from = ((y * strip.w + sx) * 4) as usize;
            let to = ((y * w + x) * 4) as usize;
            rgba[to..to + 4].copy_from_slice(&strip.rgba[from..from + 4]);
        }
    }
    UndoFace { rgba, w, h }
}

/// The name in the slot. `sink` runs 0 to 1 as the cart goes in, taking the
/// name down into the slot with it.
pub fn draw_slot_title(face: (TexId, u32), alpha: f32, sink: f32, out: &mut Vec<Draw>) {
    let sink = sink.clamp(0.0, 1.0);
    let alpha = alpha * (1.0 - ease((sink * 2.5).min(1.0)));
    if alpha <= 0.0 {
        return;
    }
    let (tex, w) = face;
    let (w, h) = (w as f32, HINT_H as f32);
    out.push(Draw::Tex {
        x: (OUT_W as f32 - w) / 2.0,
        y: slot_name_y(h) + TITLE_SINK * ease((sink * 2.5).min(1.0)),
        w,
        h,
        tex,
        alpha,
    });
}
