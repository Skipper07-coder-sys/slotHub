//! The RetroAchievements popup: an unlock slides in at the top of the screen
//! for a few seconds, one at a time, in the order they came.

use std::collections::VecDeque;

use slot_gfx::{Draw, TexId, OUT_W};

use crate::hud::Millis;
use crate::plate::UndoFace;
use crate::text::{self, Layout};

/// How long one popup is up, sliding in and out included.
pub const CHEEVO_SHOW_MS: Millis = 4000;
const SLIDE_MS: Millis = 250;
/// The most waiting; more than this many unlocks at once drop the oldest.
const QUEUE_MAX: usize = 16;

pub const CHEEVO_W: u32 = 560;
pub const CHEEVO_H: u32 = 100;
const TOP: f32 = 12.0;
const PAD: u32 = 16;

const PANEL: [u8; 3] = [0x1c, 0x1c, 0x21];
const RULE: [u8; 3] = [0xe8, 0xb4, 0x2c];
const INK: [u8; 3] = [0xf6, 0xf4, 0xef];
const DIM: [u8; 3] = [0x9a, 0x9a, 0xa4];

/// One popup's words.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Cheevo {
    pub heading: String,
    pub title: String,
    pub detail: String,
}

impl Cheevo {
    pub fn unlocked(title: &str, description: &str, points: u32) -> Cheevo {
        Cheevo {
            heading: match points {
                1 => "Achievement Unlocked, 1 Point".into(),
                n => format!("Achievement Unlocked, {n} Points"),
            },
            title: title.into(),
            detail: description.into(),
        }
    }

    /// The server's word that this ROM is not one its achievements fit.
    pub fn unsupported() -> Cheevo {
        Cheevo {
            heading: "RetroAchievements".into(),
            title: "Unsupported Game File".into(),
            detail: "No achievements for this version of the game".into(),
        }
    }

    pub fn completed(game: &str) -> Cheevo {
        Cheevo {
            heading: "Game Mastered".into(),
            title: game.into(),
            detail: "Every achievement unlocked".into(),
        }
    }
}

#[derive(Default)]
pub struct CheevoPopup {
    waiting: VecDeque<Cheevo>,
    shown: Option<(Cheevo, Millis)>,
}

impl CheevoPopup {
    pub fn push(&mut self, cheevo: Cheevo) {
        if self.waiting.len() >= QUEUE_MAX {
            self.waiting.pop_front();
        }
        self.waiting.push_back(cheevo);
    }

    /// Ends the popup whose time is up and brings on the next.
    pub fn tick(&mut self, now: Millis) {
        if let Some((_, at)) = &self.shown {
            if now.saturating_sub(*at) >= CHEEVO_SHOW_MS {
                self.shown = None;
            }
        }
        if self.shown.is_none() {
            self.shown = self.waiting.pop_front().map(|c| (c, now));
        }
    }

    pub fn current(&self) -> Option<&Cheevo> {
        self.shown.as_ref().map(|(c, _)| c)
    }

    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// How far in the popup is, from 0 (out of sight) to 1 (fully down).
    pub fn reach(&self, now: Millis) -> f32 {
        let Some((_, at)) = &self.shown else {
            return 0.0;
        };
        let t = now.saturating_sub(*at);
        let left = CHEEVO_SHOW_MS.saturating_sub(t);
        let edge = t.min(left).min(SLIDE_MS);
        let u = edge as f32 / SLIDE_MS as f32;
        1.0 - (1.0 - u) * (1.0 - u)
    }

    pub fn draw(&self, face: Option<(TexId, u32, u32)>, now: Millis, out: &mut Vec<Draw>) {
        let (Some((tex, w, h)), true) = (face, self.shown.is_some()) else {
            return;
        };
        let reach = self.reach(now);
        if reach <= 0.0 {
            return;
        }
        let y = -(h as f32) + (h as f32 + TOP) * reach;
        out.push(Draw::Tex {
            x: ((OUT_W - w) / 2) as f32,
            y: y.round(),
            w: w as f32,
            h: h as f32,
            tex,
            alpha: 1.0,
        });
    }
}

/// The popup drawn for one unlock.
pub fn cheevo_face(cheevo: &Cheevo) -> UndoFace {
    let (w, h) = (CHEEVO_W, CHEEVO_H);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    fill(&mut rgba, w, 0, 0, w, h, PANEL);
    fill(&mut rgba, w, 0, 0, 6, h, RULE);
    let text_w = w - 6 - 2 * PAD;
    line(
        &mut rgba,
        w,
        PAD + 6,
        8,
        text_w,
        24,
        &cheevo.heading,
        16.0,
        RULE,
    );
    line(
        &mut rgba,
        w,
        PAD + 6,
        34,
        text_w,
        32,
        &cheevo.title,
        26.0,
        INK,
    );
    line(
        &mut rgba,
        w,
        PAD + 6,
        68,
        text_w,
        24,
        &cheevo.detail,
        16.0,
        DIM,
    );
    UndoFace { rgba, w, h }
}

fn fill(rgba: &mut [u8], stride: u32, x: u32, y: u32, w: u32, h: u32, colour: [u8; 3]) {
    for row in y..y + h {
        for col in x..x + w {
            let i = ((row * stride + col) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 0xff]);
        }
    }
}

/// One line of type set as large as fits, up to `px`, centred in its box.
#[allow(clippy::too_many_arguments)]
fn line(
    rgba: &mut [u8],
    stride: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    s: &str,
    px: f32,
    colour: [u8; 3],
) {
    let Some(font) = text::label_font() else {
        return;
    };
    if s.is_empty() {
        return;
    }
    let mut size = px;
    while size > 10.0 && text::line_width(font, s, size, 0.0) > w as f32 {
        size -= 1.0;
    }
    let shown = clip(font, s, size, w as f32);
    let layout = Layout {
        lines: vec![shown],
        px: size,
        tracking: 0.0,
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    text::draw_centred(&mut buf, w, h, &layout, colour);
    for row in 0..h {
        for col in 0..w {
            let s = ((row * w + col) * 4) as usize;
            let a = buf[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (((y + row) * stride + x + col) * 4) as usize;
            for k in 0..3 {
                let under = rgba[d + k] as u32;
                rgba[d + k] = ((buf[s + k] as u32 * a + under * (255 - a)) / 255) as u8;
            }
        }
    }
}

/// Cuts a line too long even at the smallest size, ending it with "...".
fn clip(font: &fontdue::Font, s: &str, px: f32, w: f32) -> String {
    if text::line_width(font, s, px, 0.0) <= w {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let cut: String = chars.iter().collect::<String>().trim_end().to_string() + "...";
        if text::line_width(font, &cut, px, 0.0) <= w {
            return cut;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlocks_show_one_at_a_time_in_order() {
        let mut p = CheevoPopup::default();
        p.tick(0);
        assert_eq!(p.current(), None);
        p.push(Cheevo::unlocked("First", "a", 5));
        p.push(Cheevo::unlocked("Second", "b", 10));
        p.tick(100);
        assert_eq!(p.current().map(|c| c.title.as_str()), Some("First"));
        assert_eq!(p.waiting(), 1);
        p.tick(100 + CHEEVO_SHOW_MS - 1);
        assert_eq!(p.current().map(|c| c.title.as_str()), Some("First"));
        p.tick(100 + CHEEVO_SHOW_MS);
        assert_eq!(p.current().map(|c| c.title.as_str()), Some("Second"));
        p.tick(100 + 2 * CHEEVO_SHOW_MS);
        assert_eq!(p.current(), None);
    }

    #[test]
    fn it_slides_down_holds_and_slides_back_up() {
        let mut p = CheevoPopup::default();
        p.push(Cheevo::completed("Game"));
        p.tick(1000);
        assert_eq!(p.reach(1000), 0.0);
        assert!(p.reach(1000 + SLIDE_MS / 2) > 0.0 && p.reach(1000 + SLIDE_MS / 2) < 1.0);
        assert_eq!(p.reach(1000 + 2000), 1.0);
        assert!(p.reach(1000 + CHEEVO_SHOW_MS - SLIDE_MS / 2) < 1.0);
        let mut out = Vec::new();
        p.draw(
            Some((TexId::from_raw(7), CHEEVO_W, CHEEVO_H)),
            1000 + 2000,
            &mut out,
        );
        match out.as_slice() {
            [Draw::Tex { y, tex, .. }] => {
                assert_eq!(*tex, TexId::from_raw(7));
                assert_eq!(*y, TOP);
            }
            other => panic!("drew {other:?}"),
        }
    }

    #[test]
    fn a_flood_of_unlocks_keeps_the_newest() {
        let mut p = CheevoPopup::default();
        for i in 0..40 {
            p.push(Cheevo::unlocked(&format!("{i}"), "", 1));
        }
        assert_eq!(p.waiting(), QUEUE_MAX);
        p.tick(0);
        assert_eq!(p.current().map(|c| c.title.as_str()), Some("24"));
    }

    #[test]
    fn points_are_counted_in_the_heading() {
        assert_eq!(
            Cheevo::unlocked("t", "d", 1).heading,
            "Achievement Unlocked, 1 Point"
        );
        assert_eq!(
            Cheevo::unlocked("t", "d", 25).heading,
            "Achievement Unlocked, 25 Points"
        );
    }

    #[test]
    fn the_face_is_drawn_whole_even_for_long_words() {
        let f = cheevo_face(&Cheevo::unlocked(
            "A very long achievement title that will never fit on one line at all",
            "And a description that goes on for much longer than the popup is wide, as some do",
            100,
        ));
        assert_eq!((f.w, f.h), (CHEEVO_W, CHEEVO_H));
        assert!(f.rgba.chunks(4).all(|p| p[3] == 0xff));
    }
}
