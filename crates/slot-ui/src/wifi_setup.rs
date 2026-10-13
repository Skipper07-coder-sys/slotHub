use slot_gfx::{Draw, TexId, OUT_H, OUT_W};

use crate::keyboard::{KeyPress, Keyboard, KEY_H};
use crate::plate::{hint_face, UndoFace, HINT_H};
use crate::text::{self, Layout};
use crate::ticker::{ellipsize, overflows, Ticker};

pub(crate) const INK: [u8; 3] = [0xf6, 0xf4, 0xef];
pub(crate) const DIM: [u8; 3] = [0x9a, 0x9a, 0xa4];
pub(crate) const PAPER: [u8; 3] = [0x1c, 0x1c, 0x21];
pub(crate) const LIT: [u8; 3] = [0xf6, 0xf4, 0xef];
pub(crate) const LIT_INK: [u8; 3] = [0x12, 0x12, 0x14];
const BACKDROP: [f32; 4] = [0.06, 0.06, 0.07, 1.0];

pub(crate) const TITLE_Y: u32 = 14;
pub(crate) const TITLE_H: u32 = 44;
pub(crate) const TITLE_PX: f32 = 28.0;
pub(crate) const BODY_PX: f32 = 26.0;
pub(crate) const EDGE: u32 = 30;

pub(crate) const ROW_Y: u32 = 70;
pub(crate) const ROW_H: u32 = 44;
pub(crate) const ROW_GAP: u32 = 6;
pub(crate) const ROWS_SHOWN: usize = 7;

const HINT_Y: u32 = 444;
const HINT_GAP: u32 = 18;

pub const PSK_MIN: usize = 8;
pub const PSK_MAX: usize = 63;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub secured: bool,
    pub signal: i32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WifiSetup {
    Scanning,
    List {
        networks: Vec<WifiNetwork>,
        cursor: usize,
        /// Names the card already has a password for.
        saved: Vec<String>,
    },
    Keys {
        ssid: String,
        keys: Keyboard,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SetupKey {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    X,
    Y,
    Start,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SetupOutcome {
    Stay,
    Rescan,
    Join {
        ssid: String,
        psk: String,
    },
    /// A network the card already has the password for.
    Rejoin {
        ssid: String,
    },
    Back,
}

impl WifiSetup {
    pub fn scanned(networks: Vec<WifiNetwork>) -> WifiSetup {
        WifiSetup::scanned_with(networks, Vec::new())
    }

    /// The scan's networks, the ones in `saved` joining without a password.
    pub fn scanned_with(networks: Vec<WifiNetwork>, saved: Vec<String>) -> WifiSetup {
        WifiSetup::List {
            networks,
            cursor: 0,
            saved,
        }
    }

    pub fn password(ssid: &str) -> WifiSetup {
        WifiSetup::Keys {
            ssid: ssid.to_string(),
            keys: Keyboard::new(PSK_MAX),
        }
    }

    pub fn typed(&self) -> Option<&str> {
        match self {
            WifiSetup::Keys { keys, .. } => Some(keys.typed()),
            _ => None,
        }
    }

    pub fn press(&mut self, key: SetupKey) -> SetupOutcome {
        match self {
            WifiSetup::Scanning => match key {
                SetupKey::B => SetupOutcome::Back,
                _ => SetupOutcome::Stay,
            },
            WifiSetup::List {
                networks,
                cursor,
                saved,
            } => {
                let n = networks.len();
                match key {
                    SetupKey::Up if n > 0 => *cursor = (*cursor + n - 1) % n,
                    SetupKey::Down if n > 0 => *cursor = (*cursor + 1) % n,
                    SetupKey::X => return SetupOutcome::Rescan,
                    SetupKey::B => return SetupOutcome::Back,
                    SetupKey::A => {
                        let Some(net) = networks.get(*cursor) else {
                            return SetupOutcome::Rescan;
                        };
                        if saved.contains(&net.ssid) {
                            return SetupOutcome::Rejoin {
                                ssid: net.ssid.clone(),
                            };
                        }
                        if !net.secured {
                            return SetupOutcome::Join {
                                ssid: net.ssid.clone(),
                                psk: String::new(),
                            };
                        }
                        *self = WifiSetup::password(&net.ssid);
                    }
                    // A saved network whose password has changed.
                    SetupKey::Y => match networks.get(*cursor) {
                        Some(net) if net.secured && saved.contains(&net.ssid) => {
                            *self = WifiSetup::password(&net.ssid);
                        }
                        _ => {}
                    },
                    _ => {}
                }
                SetupOutcome::Stay
            }
            WifiSetup::Keys { ssid, keys } => match keys.press(key) {
                KeyPress::Stay => SetupOutcome::Stay,
                KeyPress::Empty => SetupOutcome::Rescan,
                KeyPress::Done => done(ssid, keys.typed()),
            },
        }
    }

    pub fn face(&self) -> UndoFace {
        let mut canvas = Canvas::new();
        match self {
            WifiSetup::Scanning => {
                canvas.text_centred("Wi-Fi Networks", TITLE_Y, TITLE_H, TITLE_PX, INK);
                canvas.text_centred("Looking for networks...", 200, 48, BODY_PX, DIM);
                canvas.hints(&[("B", "Back")]);
            }
            WifiSetup::List {
                networks,
                cursor,
                saved,
            } => {
                canvas.text_centred("Wi-Fi Networks", TITLE_Y, TITLE_H, TITLE_PX, INK);
                if networks.is_empty() {
                    canvas.text_centred("No networks found", 200, 48, BODY_PX, DIM);
                }
                let first = cursor.saturating_sub(ROWS_SHOWN - 1);
                for (i, net) in networks.iter().enumerate().skip(first).take(ROWS_SHOWN) {
                    let y = ROW_Y + (i - first) as u32 * (ROW_H + ROW_GAP);
                    let lit = i == *cursor;
                    let (bg, ink) = if lit { (LIT, LIT_INK) } else { (PAPER, INK) };
                    canvas.fill(EDGE, y, OUT_W - 2 * EDGE, ROW_H, bg);
                    let right = right_text(net, saved);
                    let rw = canvas.text_right(&right, OUT_W - EDGE - 14, y, ROW_H, BODY_PX, ink);
                    canvas.list_name(&net.ssid, rw, y, lit, ink);
                }
                match networks.get(*cursor) {
                    Some(net) if net.secured && saved.contains(&net.ssid) => canvas.hints(&[
                        ("B", "Back"),
                        ("X", "Rescan"),
                        ("Y", "New Password"),
                        ("A", "Join"),
                    ]),
                    _ => canvas.hints(&[("B", "Back"), ("X", "Rescan"), ("A", "Join")]),
                }
            }
            WifiSetup::Keys { ssid, keys } => {
                let title = format!("Password for {ssid}");
                canvas.text_centred(&title, TITLE_Y, TITLE_H, TITLE_PX, INK);
                keys.draw(&mut canvas, "Join");
                canvas.hints(&[
                    ("B", "Delete"),
                    ("X", "ABC #+="),
                    ("Y", "Space"),
                    ("START", "Join"),
                ]);
            }
        }
        canvas.face()
    }

    /// The lit network's name when it is too long for its row, to scroll.
    pub fn ticker(&self) -> Option<Ticker> {
        let WifiSetup::List {
            networks,
            cursor,
            saved,
        } = self
        else {
            return None;
        };
        let net = networks.get(*cursor)?;
        let right = right_text(net, saved);
        name_ticker(&net.ssid, &right, *cursor)
    }

    pub fn draw(face: Option<TexId>, out: &mut Vec<Draw>) {
        out.push(Draw::Rect {
            x: 0.0,
            y: 0.0,
            w: OUT_W as f32,
            h: OUT_H as f32,
            colour: BACKDROP,
        });
        if let Some(tex) = face {
            out.push(Draw::Tex {
                x: 0.0,
                y: 0.0,
                w: OUT_W as f32,
                h: OUT_H as f32,
                tex,
                alpha: 1.0,
            });
        }
    }
}

const NAME_X: u32 = EDGE + 14;

// The room a list row leaves its name beside the text on its right.
fn name_room(right_w: u32) -> u32 {
    OUT_W - 2 * EDGE - 28 - right_w - 16
}

/// A list row's name that is too long for it, as a ticker over the lit row.
/// `index` is the row's place in the whole list.
pub(crate) fn name_ticker(name: &str, right: &str, index: usize) -> Option<Ticker> {
    let font = text::label_font()?;
    let right_w = text::line_width(font, right, BODY_PX, 0.0).ceil() as u32 + 2;
    let room = name_room(right_w) - 2;
    if !overflows(name, BODY_PX, 0.0, room as f32) {
        return None;
    }
    let first = index.saturating_sub(ROWS_SHOWN - 1);
    Some(Ticker {
        text: name.to_string(),
        px: BODY_PX,
        tracking: 0.0,
        ink: LIT_INK,
        x: NAME_X as f32,
        y: (ROW_Y + (index - first) as u32 * (ROW_H + ROW_GAP)) as f32,
        room,
        h: ROW_H,
    })
}

fn done(ssid: &str, typed: &str) -> SetupOutcome {
    if typed.chars().count() < PSK_MIN {
        return SetupOutcome::Stay;
    }
    SetupOutcome::Join {
        ssid: ssid.to_string(),
        psk: typed.to_string(),
    }
}

pub(crate) fn bars(signal: i32) -> &'static str {
    match signal {
        s if s >= -55 => "||||",
        s if s >= -67 => "|||",
        s if s >= -78 => "||",
        _ => "|",
    }
}

fn right_text(net: &WifiNetwork, saved: &[String]) -> String {
    let kind = match saved.contains(&net.ssid) {
        true => "Saved",
        false => lock(net.secured),
    };
    format!("{} {kind}", bars(net.signal))
}

fn lock(secured: bool) -> &'static str {
    if secured {
        "Locked"
    } else {
        "Open"
    }
}

pub(crate) struct Canvas {
    rgba: Vec<u8>,
}

impl Canvas {
    pub(crate) fn new() -> Self {
        Canvas {
            rgba: vec![0u8; (OUT_W * OUT_H * 4) as usize],
        }
    }

    pub(crate) fn face(self) -> UndoFace {
        UndoFace {
            rgba: self.rgba,
            w: OUT_W,
            h: OUT_H,
        }
    }

    pub(crate) fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, colour: [u8; 3]) {
        for row in y..(y + h).min(OUT_H) {
            for col in x..(x + w).min(OUT_W) {
                let i = ((row * OUT_W + col) * 4) as usize;
                self.rgba[i..i + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 0xff]);
            }
        }
    }

    // Draws over what is already there, keeping a filled box behind the type.
    fn stamp(&mut self, buf: &[u8], w: u32, h: u32, x: u32, y: u32) {
        for row in 0..h {
            for col in 0..w {
                let (dx, dy) = (x + col, y + row);
                if dx >= OUT_W || dy >= OUT_H {
                    continue;
                }
                let s = ((row * w + col) * 4) as usize;
                let a = buf[s + 3] as u32;
                if a == 0 {
                    continue;
                }
                let d = ((dy * OUT_W + dx) * 4) as usize;
                for k in 0..3 {
                    let under = self.rgba[d + k] as u32;
                    self.rgba[d + k] = ((buf[s + k] as u32 * a + under * (255 - a)) / 255) as u8;
                }
                self.rgba[d + 3] = self.rgba[d + 3].max(buf[s + 3]);
            }
        }
    }

    fn line(&mut self, s: &str, px: f32, max_w: u32, h: u32, colour: [u8; 3]) -> (Vec<u8>, u32) {
        let Some(font) = text::label_font() else {
            return (Vec::new(), 0);
        };
        let mut px = px;
        while px > 14.0 && text::line_width(font, s, px, 0.0) > max_w as f32 {
            px -= 1.0;
        }
        let w = (text::line_width(font, s, px, 0.0).ceil() as u32 + 2).min(max_w.max(1));
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let layout = Layout {
            lines: vec![s.to_string()],
            px,
            tracking: 0.0,
        };
        text::draw_centred(&mut buf, w, h, &layout, colour);
        (buf, w)
    }

    /// A list row's name, cut with an ellipsis when it is too long. The lit
    /// row leaves a long name out, for its ticker to scroll there.
    pub(crate) fn list_name(&mut self, name: &str, right_w: u32, y: u32, lit: bool, ink: [u8; 3]) {
        let room = name_room(right_w) - 2;
        if !overflows(name, BODY_PX, 0.0, room as f32) {
            self.text_left(name, NAME_X, y, room + 2, ROW_H, ink);
        } else if !lit {
            let cut = ellipsize(name, BODY_PX, 0.0, room as f32);
            self.text_left(&cut, NAME_X, y, room + 2, ROW_H, ink);
        }
    }

    pub(crate) fn text_centred(&mut self, s: &str, y: u32, h: u32, px: f32, colour: [u8; 3]) {
        let (buf, w) = self.line(s, px, OUT_W - 2 * EDGE, h, colour);
        self.stamp(&buf, w, h, (OUT_W - w) / 2, y);
    }

    pub(crate) fn text_left(&mut self, s: &str, x: u32, y: u32, max_w: u32, h: u32, ink: [u8; 3]) {
        let (buf, w) = self.line(s, BODY_PX, max_w, h, ink);
        self.stamp(&buf, w, h, x, y);
    }

    pub(crate) fn text_right(
        &mut self,
        s: &str,
        right: u32,
        y: u32,
        h: u32,
        px: f32,
        ink: [u8; 3],
    ) -> u32 {
        let (buf, w) = self.line(s, px, OUT_W, h, ink);
        self.stamp(&buf, w, h, right - w, y);
        w
    }

    // Long passwords keep their end in view, where the typing happens.
    pub(crate) fn text_tail(&mut self, s: &str, x: u32, y: u32, max_w: u32, h: u32, ink: [u8; 3]) {
        let Some(font) = text::label_font() else {
            return;
        };
        let chars: Vec<char> = s.chars().collect();
        let mut from = 0;
        while from < chars.len() {
            let tail: String = chars[from..].iter().collect();
            if text::line_width(font, &tail, BODY_PX, 0.0) <= max_w as f32 {
                break;
            }
            from += 1;
        }
        let tail: String = chars[from..].iter().collect();
        self.text_left(&tail, x, y, max_w, h, ink);
    }

    pub(crate) fn key(&mut self, label: &str, x: u32, y: u32, w: u32, lit: bool) {
        let (bg, ink) = if lit { (LIT, LIT_INK) } else { (PAPER, INK) };
        self.fill(x, y, w, KEY_H, bg);
        let (buf, bw) = self.line(label, BODY_PX, w - 4, KEY_H, ink);
        self.stamp(&buf, bw, KEY_H, x + (w - bw) / 2, y);
    }

    pub(crate) fn hints(&mut self, hints: &[(&str, &str)]) {
        let faces: Vec<UndoFace> = hints.iter().map(|(k, l)| hint_face(k, l)).collect();
        let total: u32 = faces.iter().map(|f| f.w).sum::<u32>()
            + HINT_GAP * faces.len().saturating_sub(1) as u32;
        let mut x = OUT_W.saturating_sub(total) / 2;
        for f in faces {
            if x + f.w > OUT_W || f.h != HINT_H {
                break;
            }
            self.stamp(&f.rgba, f.w, HINT_H, x, HINT_Y);
            x += f.w + HINT_GAP;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets() -> Vec<WifiNetwork> {
        vec![
            WifiNetwork {
                ssid: "Home".into(),
                secured: true,
                signal: -50,
            },
            WifiNetwork {
                ssid: "Cafe".into(),
                secured: false,
                signal: -80,
            },
        ]
    }

    #[test]
    fn an_open_network_joins_straight_from_the_list() {
        let mut s = WifiSetup::scanned(nets());
        s.press(SetupKey::Down);
        assert_eq!(
            s.press(SetupKey::A),
            SetupOutcome::Join {
                ssid: "Cafe".into(),
                psk: String::new()
            }
        );
    }

    #[test]
    fn a_locked_network_asks_for_its_password_and_joins_with_what_was_typed() {
        let mut s = WifiSetup::scanned(nets());
        assert_eq!(s.press(SetupKey::A), SetupOutcome::Stay);
        assert_eq!(s.typed(), Some(""));
        // q w e r t y u i: the second row of the first page
        s.press(SetupKey::Down);
        for _ in 0..8 {
            s.press(SetupKey::A);
            s.press(SetupKey::Right);
        }
        assert_eq!(s.typed(), Some("qwertyui"));
        s.press(SetupKey::X);
        s.press(SetupKey::Left);
        s.press(SetupKey::A);
        assert_eq!(s.typed(), Some("qwertyuiI"));
        s.press(SetupKey::B);
        assert_eq!(
            s.press(SetupKey::Start),
            SetupOutcome::Join {
                ssid: "Home".into(),
                psk: "qwertyui".into()
            }
        );
    }

    #[test]
    fn a_password_too_short_for_wpa_is_not_sent() {
        let mut s = WifiSetup::password("Home");
        s.press(SetupKey::A);
        assert_eq!(s.press(SetupKey::Start), SetupOutcome::Stay);
    }

    #[test]
    fn the_bottom_row_holds_space_delete_and_join() {
        let mut s = WifiSetup::password("Home");
        s.press(SetupKey::Up);
        s.press(SetupKey::Right);
        s.press(SetupKey::A);
        assert_eq!(s.typed(), Some(" "));
        s.press(SetupKey::Right);
        s.press(SetupKey::A);
        assert_eq!(s.typed(), Some(""));
    }

    #[test]
    fn b_on_an_empty_password_goes_back_to_the_list() {
        let mut s = WifiSetup::password("Home");
        assert_eq!(s.press(SetupKey::B), SetupOutcome::Rescan);
    }

    #[test]
    fn the_cursor_stays_on_a_key_moving_onto_a_short_row() {
        let mut s = WifiSetup::password("Home");
        s.press(SetupKey::X);
        s.press(SetupKey::X);
        for _ in 0..9 {
            s.press(SetupKey::Right);
        }
        for _ in 0..3 {
            s.press(SetupKey::Down);
        }
        s.press(SetupKey::A);
        assert_eq!(s.typed(), Some("?"));
    }

    #[test]
    fn every_screen_draws_a_full_frame() {
        for s in [
            WifiSetup::Scanning,
            WifiSetup::scanned(Vec::new()),
            WifiSetup::scanned(nets()),
            WifiSetup::password("A rather long network name that will not fit"),
        ] {
            let f = s.face();
            assert_eq!((f.w, f.h), (OUT_W, OUT_H));
            assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "{s:?} drew nothing");
        }
    }
}
