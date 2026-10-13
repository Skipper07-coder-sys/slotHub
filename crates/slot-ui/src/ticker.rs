use crate::plate::UndoFace;
use crate::text::{self, Layout};

/// How long a long line rests at its start before it scrolls.
pub const TICKER_HOLD_MS: f64 = 1500.0;
/// Pixels a second the line moves right to left.
pub const TICKER_SPEED: f64 = 60.0;
/// Space between the line's end and its next pass coming in from the right.
pub const TICKER_GAP: u32 = 72;
const FADE: u32 = 18;

/// A line too long for the room it has, which scrolls right to left there
/// instead of being cut off.
#[derive(Clone, PartialEq, Debug)]
pub struct Ticker {
    pub text: String,
    pub px: f32,
    pub tracking: f32,
    pub ink: [u8; 3],
    pub x: f32,
    pub y: f32,
    pub room: u32,
    pub h: u32,
}

/// The whole line, drawn once, that each frame shows a window of.
pub fn ticker_strip(t: &Ticker) -> UndoFace {
    let Some(font) = text::label_font() else {
        return UndoFace {
            rgba: Vec::new(),
            w: 0,
            h: 0,
        };
    };
    let w = text::line_width(font, &t.text, t.px, t.tracking).ceil() as u32 + 2;
    let mut rgba = vec![0u8; (w * t.h * 4) as usize];
    let layout = Layout {
        lines: vec![t.text.clone()],
        px: t.px,
        tracking: t.tracking,
    };
    text::draw_centred(&mut rgba, w, t.h, &layout, t.ink);
    UndoFace { rgba, w, h: t.h }
}

/// How far the line has moved after `ms`: still at first, then steadily left
/// until its next pass sits where it began, then still again.
pub fn ticker_offset(strip_w: u32, ms: f64) -> f32 {
    let cycle = (strip_w + TICKER_GAP) as f64;
    let moving = cycle / TICKER_SPEED * 1000.0;
    let t = ms.max(0.0) % (TICKER_HOLD_MS + moving);
    match t < TICKER_HOLD_MS {
        true => 0.0,
        false => ((t - TICKER_HOLD_MS) / 1000.0 * TICKER_SPEED) as f32,
    }
}

/// The `room` wide window of the line `ms` after it was first shown, faded
/// at the edge the text runs past.
pub fn ticker_frame(strip: &UndoFace, room: u32, ms: f64) -> UndoFace {
    let h = strip.h;
    let mut rgba = vec![0u8; (room * h * 4) as usize];
    if strip.w == 0 || room == 0 {
        return UndoFace { rgba, w: room, h };
    }
    let cycle = strip.w + TICKER_GAP;
    let off = ticker_offset(strip.w, ms);
    let shift = off.floor() as u32;
    let frac = off - off.floor();
    // The left fade grows as the start leaves and shrinks as the next pass
    // arrives, so the rest between passes begins unfaded.
    let away = (off / FADE as f32)
        .min((cycle as f32 - off) / FADE as f32)
        .clamp(0.0, 1.0);
    let sample = |col: u32, row: u32| -> [f32; 4] {
        let src = (col + shift) % cycle;
        if src >= strip.w {
            return [0.0; 4];
        }
        let at = ((row * strip.w + src) * 4) as usize;
        let p = &strip.rgba[at..at + 4];
        [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]
    };
    for row in 0..h {
        for col in 0..room {
            let (a, b) = (sample(col, row), sample(col + 1, row));
            let mut px = [0.0f32; 4];
            for k in 0..4 {
                px[k] = a[k] * (1.0 - frac) + b[k] * frac;
            }
            let left = match col < FADE {
                true => 1.0 - away * (1.0 - col as f32 / FADE as f32),
                false => 1.0,
            };
            let right = ((room - col) as f32 / FADE as f32).min(1.0);
            let at = ((row * room + col) * 4) as usize;
            let ink = if a[3] >= b[3] { a } else { b };
            rgba[at..at + 3].copy_from_slice(&[ink[0] as u8, ink[1] as u8, ink[2] as u8]);
            rgba[at + 3] = (px[3] * left * right).round() as u8;
        }
    }
    UndoFace { rgba, w: room, h }
}

/// `text` cut to fit `room` with an ellipsis, for a line that is not the
/// one being read.
pub fn ellipsize(text_in: &str, px: f32, tracking: f32, room: f32) -> String {
    let Some(font) = text::label_font() else {
        return text_in.to_string();
    };
    if text::line_width(font, text_in, px, tracking) <= room {
        return text_in.to_string();
    }
    let chars: Vec<char> = text_in.chars().collect();
    let mut keep = chars.len();
    while keep > 0 {
        let cut: String = chars[..keep].iter().collect();
        let line = format!("{}...", cut.trim_end());
        if text::line_width(font, &line, px, tracking) <= room {
            return line;
        }
        keep -= 1;
    }
    "...".into()
}

/// Whether `text` needs more than `room` at this size.
pub fn overflows(text_in: &str, px: f32, tracking: f32, room: f32) -> bool {
    text::label_font().is_some_and(|font| text::line_width(font, text_in, px, tracking) > room)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_rests_then_moves_left_and_comes_round_to_rest_again() {
        assert_eq!(ticker_offset(300, 0.0), 0.0);
        assert_eq!(ticker_offset(300, TICKER_HOLD_MS - 1.0), 0.0);
        let a = ticker_offset(300, TICKER_HOLD_MS + 500.0);
        let b = ticker_offset(300, TICKER_HOLD_MS + 1000.0);
        assert!(a > 0.0 && b > a, "{a} {b}");
        let pass = (300 + TICKER_GAP) as f64 / TICKER_SPEED * 1000.0;
        assert_eq!(ticker_offset(300, TICKER_HOLD_MS + pass + 1.0), 0.0);
    }

    #[test]
    fn a_frame_is_the_room_wide_and_the_strip_high() {
        let strip = UndoFace {
            rgba: vec![255; 40 * 10 * 4],
            w: 40,
            h: 10,
        };
        let f = ticker_frame(&strip, 25, 0.0);
        assert_eq!((f.w, f.h, f.rgba.len()), (25, 10, 25 * 10 * 4));
        // At rest the start is not faded; the edge the text runs past is.
        assert_eq!(f.rgba[3], 255);
        assert!(f.rgba[(24 * 4 + 3) as usize] < 255);
    }

    #[test]
    fn a_name_that_fits_is_kept_and_one_that_does_not_ends_in_an_ellipsis() {
        if text::label_font().is_none() {
            return;
        }
        assert_eq!(ellipsize("Home", 26.0, 0.0, 400.0), "Home");
        let cut = ellipsize("A Very Long Network Name Indeed", 26.0, 0.0, 150.0);
        assert!(cut.ends_with("..."), "{cut}");
        assert!(!overflows(&cut, 26.0, 0.0, 150.0), "{cut}");
    }
}
