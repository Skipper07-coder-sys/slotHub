use slot_gfx::{Draw, TexId, OUT_H, OUT_W};

use crate::plate::{hint_face, UndoFace, HINT_H};
use crate::text::{self, Layout};

const INK: [u8; 3] = [0xf6, 0xf4, 0xef];
const DIM: [u8; 3] = [0x9a, 0x9a, 0xa4];
const PAPER: [u8; 3] = [0x1c, 0x1c, 0x21];
const LIT: [u8; 3] = [0xf6, 0xf4, 0xef];
const LIT_INK: [u8; 3] = [0x12, 0x12, 0x14];
const BACKDROP: [f32; 4] = [0.06, 0.06, 0.07, 1.0];

const TITLE_Y: u32 = 14;
const TITLE_H: u32 = 44;
const TITLE_PX: f32 = 28.0;
const BODY_PX: f32 = 26.0;
const EDGE: u32 = 30;

const ROW_Y: u32 = 70;
const ROW_H: u32 = 44;
const ROW_GAP: u32 = 6;
const ROWS_SHOWN: usize = 7;

const FIELD_Y: u32 = 64;
const FIELD_H: u32 = 48;
const GRID_Y: u32 = 124;
const KEY_W: u32 = 60;
const KEY_H: u32 = 50;
const KEY_GAP: u32 = 6;
const COLS: usize = 10;
const ACTIONS: [Action; 4] = [Action::Page, Action::Space, Action::Delete, Action::Done];

const HINT_Y: u32 = 444;
const HINT_GAP: u32 = 18;

pub const PSK_MIN: usize = 8;
pub const PSK_MAX: usize = 63;

const PAGES: [[&str; 4]; 3] = [
    ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.-"],
    ["1234567890", "QWERTYUIOP", "ASDFGHJKL\"", "ZXCVBNM<>_"],
    ["!@#$%^&*()", "`~-_=+[]{}", "\\|;:'\",.<>", "/?"],
];

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub secured: bool,
    pub signal: i32,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Action {
    Page,
    Space,
    Delete,
    Done,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WifiSetup {
    Scanning,
    List {
        networks: Vec<WifiNetwork>,
        cursor: usize,
    },
    Keys {
        ssid: String,
        typed: String,
        page: usize,
        row: usize,
        col: usize,
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
    Join { ssid: String, psk: String },
    Back,
}

impl WifiSetup {
    pub fn scanned(networks: Vec<WifiNetwork>) -> WifiSetup {
        WifiSetup::List {
            networks,
            cursor: 0,
        }
    }

    pub fn password(ssid: &str) -> WifiSetup {
        WifiSetup::Keys {
            ssid: ssid.to_string(),
            typed: String::new(),
            page: 0,
            row: 0,
            col: 0,
        }
    }

    pub fn typed(&self) -> Option<&str> {
        match self {
            WifiSetup::Keys { typed, .. } => Some(typed),
            _ => None,
        }
    }

    pub fn press(&mut self, key: SetupKey) -> SetupOutcome {
        match self {
            WifiSetup::Scanning => match key {
                SetupKey::B => SetupOutcome::Back,
                _ => SetupOutcome::Stay,
            },
            WifiSetup::List { networks, cursor } => {
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
                        if !net.secured {
                            return SetupOutcome::Join {
                                ssid: net.ssid.clone(),
                                psk: String::new(),
                            };
                        }
                        *self = WifiSetup::password(&net.ssid);
                    }
                    _ => {}
                }
                SetupOutcome::Stay
            }
            WifiSetup::Keys {
                ssid,
                typed,
                page,
                row,
                col,
            } => {
                let rows = PAGES[*page].len();
                match key {
                    SetupKey::Up => *row = (*row + rows) % (rows + 1),
                    SetupKey::Down => *row = (*row + 1) % (rows + 1),
                    SetupKey::Left => {
                        let w = row_len(*page, *row);
                        *col = ((*col).min(w - 1) + w - 1) % w;
                    }
                    SetupKey::Right => {
                        let w = row_len(*page, *row);
                        *col = ((*col).min(w - 1) + 1) % w;
                    }
                    SetupKey::B => {
                        if typed.pop().is_none() {
                            return SetupOutcome::Rescan;
                        }
                    }
                    SetupKey::X => *page = (*page + 1) % PAGES.len(),
                    SetupKey::Y => push(typed, ' '),
                    SetupKey::Start => return done(ssid, typed),
                    SetupKey::A => {
                        if *row == rows {
                            match ACTIONS[(*col).min(ACTIONS.len() - 1)] {
                                Action::Page => *page = (*page + 1) % PAGES.len(),
                                Action::Space => push(typed, ' '),
                                Action::Delete => {
                                    typed.pop();
                                }
                                Action::Done => return done(ssid, typed),
                            }
                        } else if let Some(c) = PAGES[*page][*row].chars().nth(*col) {
                            push(typed, c);
                        }
                    }
                }
                *col = (*col).min(row_len(*page, *row) - 1);
                SetupOutcome::Stay
            }
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
            WifiSetup::List { networks, cursor } => {
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
                    let right = format!("{} {}", bars(net.signal), lock(net.secured));
                    let rw = canvas.text_right(&right, OUT_W - EDGE - 14, y, ROW_H, BODY_PX, ink);
                    let room = OUT_W - 2 * EDGE - 28 - rw - 16;
                    canvas.text_left(&net.ssid, EDGE + 14, y, room, ROW_H, ink);
                }
                canvas.hints(&[("B", "Back"), ("X", "Rescan"), ("A", "Join")]);
            }
            WifiSetup::Keys {
                ssid,
                typed,
                page,
                row,
                col,
            } => {
                let title = format!("Password for {ssid}");
                canvas.text_centred(&title, TITLE_Y, TITLE_H, TITLE_PX, INK);
                canvas.fill(EDGE, FIELD_Y, OUT_W - 2 * EDGE, FIELD_H, PAPER);
                let shown = format!("{typed}_");
                canvas.text_tail(
                    &shown,
                    EDGE + 14,
                    FIELD_Y,
                    OUT_W - 2 * EDGE - 28,
                    FIELD_H,
                    INK,
                );
                let grid_w = COLS as u32 * (KEY_W + KEY_GAP) - KEY_GAP;
                let x0 = (OUT_W - grid_w) / 2;
                for (r, keys) in PAGES[*page].iter().enumerate() {
                    for (c, ch) in keys.chars().enumerate() {
                        let x = x0 + c as u32 * (KEY_W + KEY_GAP);
                        let y = GRID_Y + r as u32 * (KEY_H + KEY_GAP);
                        let lit = r == *row && c == *col;
                        canvas.key(&ch.to_string(), x, y, KEY_W, lit);
                    }
                }
                let action_y = GRID_Y + PAGES[*page].len() as u32 * (KEY_H + KEY_GAP);
                let wide = (grid_w - KEY_GAP * (ACTIONS.len() as u32 - 1)) / ACTIONS.len() as u32;
                for (i, action) in ACTIONS.iter().enumerate() {
                    let x = x0 + i as u32 * (wide + KEY_GAP);
                    let lit = *row == PAGES[*page].len() && *col == i;
                    let label = match action {
                        Action::Page => ["ABC", "#+=", "abc"][*page],
                        Action::Space => "Space",
                        Action::Delete => "Delete",
                        Action::Done => "Join",
                    };
                    canvas.key(label, x, action_y, wide, lit);
                }
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

fn row_len(page: usize, row: usize) -> usize {
    PAGES[page]
        .get(row)
        .map_or(ACTIONS.len(), |keys| keys.chars().count())
}

fn push(typed: &mut String, c: char) {
    if typed.chars().count() < PSK_MAX {
        typed.push(c);
    }
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

fn bars(signal: i32) -> &'static str {
    match signal {
        s if s >= -55 => "||||",
        s if s >= -67 => "|||",
        s if s >= -78 => "||",
        _ => "|",
    }
}

fn lock(secured: bool) -> &'static str {
    if secured {
        "Locked"
    } else {
        "Open"
    }
}

struct Canvas {
    rgba: Vec<u8>,
}

impl Canvas {
    fn new() -> Self {
        Canvas {
            rgba: vec![0u8; (OUT_W * OUT_H * 4) as usize],
        }
    }

    fn face(self) -> UndoFace {
        UndoFace {
            rgba: self.rgba,
            w: OUT_W,
            h: OUT_H,
        }
    }

    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, colour: [u8; 3]) {
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

    fn text_centred(&mut self, s: &str, y: u32, h: u32, px: f32, colour: [u8; 3]) {
        let (buf, w) = self.line(s, px, OUT_W - 2 * EDGE, h, colour);
        self.stamp(&buf, w, h, (OUT_W - w) / 2, y);
    }

    fn text_left(&mut self, s: &str, x: u32, y: u32, max_w: u32, h: u32, ink: [u8; 3]) {
        let (buf, w) = self.line(s, BODY_PX, max_w, h, ink);
        self.stamp(&buf, w, h, x, y);
    }

    fn text_right(&mut self, s: &str, right: u32, y: u32, h: u32, px: f32, ink: [u8; 3]) -> u32 {
        let (buf, w) = self.line(s, px, OUT_W, h, ink);
        self.stamp(&buf, w, h, right - w, y);
        w
    }

    // Long passwords keep their end in view, where the typing happens.
    fn text_tail(&mut self, s: &str, x: u32, y: u32, max_w: u32, h: u32, ink: [u8; 3]) {
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

    fn key(&mut self, label: &str, x: u32, y: u32, w: u32, lit: bool) {
        let (bg, ink) = if lit { (LIT, LIT_INK) } else { (PAPER, INK) };
        self.fill(x, y, w, KEY_H, bg);
        let (buf, bw) = self.line(label, BODY_PX, w - 4, KEY_H, ink);
        self.stamp(&buf, bw, KEY_H, x + (w - bw) / 2, y);
    }

    fn hints(&mut self, hints: &[(&str, &str)]) {
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
