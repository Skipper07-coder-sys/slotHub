//! The on-screen keyboard: three pages of keys and a row of actions, moved
//! over with the d-pad. Wi-Fi passwords and RetroAchievements sign-in use it.

use slot_gfx::OUT_W;

use crate::wifi_setup::{Canvas, SetupKey, EDGE, INK, PAPER};

const FIELD_Y: u32 = 64;
const FIELD_H: u32 = 48;
const GRID_Y: u32 = 124;
pub(crate) const KEY_W: u32 = 60;
pub(crate) const KEY_H: u32 = 50;
const KEY_GAP: u32 = 6;
const COLS: usize = 10;
const ACTIONS: [Action; 4] = [Action::Page, Action::Space, Action::Delete, Action::Done];

const PAGES: [[&str; 4]; 3] = [
    ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.-"],
    ["1234567890", "QWERTYUIOP", "ASDFGHJKL\"", "ZXCVBNM<>_"],
    ["!@#$%^&*()", "`~-_=+[]{}", "\\|;:'\",.<>", "/?"],
];

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Action {
    Page,
    Space,
    Delete,
    Done,
}

/// What a press on the keyboard came to.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KeyPress {
    Stay,
    /// START, or the Done key.
    Done,
    /// B with nothing left to delete.
    Empty,
}

/// What has been typed and where the cursor is. A masked keyboard shows
/// stars in place of what was typed and never prints it; either way it is
/// wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Keyboard {
    typed: String,
    page: usize,
    row: usize,
    col: usize,
    max: usize,
    masked: bool,
}

impl std::fmt::Debug for Keyboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Keyboard");
        match self.masked {
            true => s.field(
                "typed",
                &format_args!("{} hidden", self.typed.chars().count()),
            ),
            false => s.field("typed", &self.typed),
        };
        s.field("page", &self.page)
            .field("row", &self.row)
            .field("col", &self.col)
            .finish()
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.typed).into_bytes();
        for b in bytes.iter_mut() {
            // Volatile so the write is not dropped as dead before the free.
            unsafe { std::ptr::write_volatile(b, 0) };
        }
    }
}

impl Keyboard {
    pub fn new(max: usize) -> Keyboard {
        Keyboard {
            // Every key is ASCII: room for all of it up front means no
            // copies of a half-typed password are left behind by growing.
            typed: String::with_capacity(max),
            page: 0,
            row: 0,
            col: 0,
            max,
            masked: false,
        }
    }

    /// A keyboard with `text` already typed.
    pub fn with(max: usize, text: &str) -> Keyboard {
        let mut k = Keyboard::new(max);
        text.chars().for_each(|c| k.push(c));
        k
    }

    pub fn masked(max: usize) -> Keyboard {
        let mut k = Keyboard::new(max);
        k.masked = true;
        k
    }

    pub fn typed(&self) -> &str {
        &self.typed
    }

    /// Moves what was typed out, leaving the keyboard empty.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.typed)
    }

    pub fn press(&mut self, key: SetupKey) -> KeyPress {
        let rows = PAGES[self.page].len();
        match key {
            SetupKey::Up => self.row = (self.row + rows) % (rows + 1),
            SetupKey::Down => self.row = (self.row + 1) % (rows + 1),
            SetupKey::Left => {
                let w = row_len(self.page, self.row);
                self.col = (self.col.min(w - 1) + w - 1) % w;
            }
            SetupKey::Right => {
                let w = row_len(self.page, self.row);
                self.col = (self.col.min(w - 1) + 1) % w;
            }
            SetupKey::B => {
                if self.typed.pop().is_none() {
                    return KeyPress::Empty;
                }
            }
            SetupKey::X => self.page = (self.page + 1) % PAGES.len(),
            SetupKey::Y => self.push(' '),
            SetupKey::Start => return KeyPress::Done,
            SetupKey::A => {
                if self.row == rows {
                    match ACTIONS[self.col.min(ACTIONS.len() - 1)] {
                        Action::Page => self.page = (self.page + 1) % PAGES.len(),
                        Action::Space => self.push(' '),
                        Action::Delete => {
                            self.typed.pop();
                        }
                        Action::Done => return KeyPress::Done,
                    }
                } else if let Some(c) = PAGES[self.page][self.row].chars().nth(self.col) {
                    self.push(c);
                }
            }
        }
        self.col = self.col.min(row_len(self.page, self.row) - 1);
        KeyPress::Stay
    }

    fn push(&mut self, c: char) {
        if self.typed.chars().count() < self.max {
            self.typed.push(c);
        }
    }

    /// The field, the keys and the action row, `done` naming the last key.
    pub(crate) fn draw(&self, canvas: &mut Canvas, done: &str) {
        canvas.fill(EDGE, FIELD_Y, OUT_W - 2 * EDGE, FIELD_H, PAPER);
        let shown = match self.masked {
            true => "*".repeat(self.typed.chars().count()) + "_",
            false => format!("{}_", self.typed),
        };
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
        for (r, keys) in PAGES[self.page].iter().enumerate() {
            for (c, ch) in keys.chars().enumerate() {
                let x = x0 + c as u32 * (KEY_W + KEY_GAP);
                let y = GRID_Y + r as u32 * (KEY_H + KEY_GAP);
                let lit = r == self.row && c == self.col;
                canvas.key(&ch.to_string(), x, y, KEY_W, lit);
            }
        }
        let action_y = GRID_Y + PAGES[self.page].len() as u32 * (KEY_H + KEY_GAP);
        let wide = (grid_w - KEY_GAP * (ACTIONS.len() as u32 - 1)) / ACTIONS.len() as u32;
        for (i, action) in ACTIONS.iter().enumerate() {
            let x = x0 + i as u32 * (wide + KEY_GAP);
            let lit = self.row == PAGES[self.page].len() && self.col == i;
            let label = match action {
                Action::Page => ["ABC", "#+=", "abc"][self.page],
                Action::Space => "Space",
                Action::Delete => "Delete",
                Action::Done => done,
            };
            canvas.key(label, x, action_y, wide, lit);
        }
    }
}

fn row_len(page: usize, row: usize) -> usize {
    PAGES[page]
        .get(row)
        .map_or(ACTIONS.len(), |keys| keys.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_stops_at_the_most_it_may_hold() {
        let mut k = Keyboard::new(3);
        for _ in 0..5 {
            k.press(SetupKey::A);
        }
        assert_eq!(k.typed(), "111");
    }

    #[test]
    fn b_deletes_then_says_there_is_nothing_left() {
        let mut k = Keyboard::new(10);
        k.press(SetupKey::A);
        assert_eq!(k.press(SetupKey::B), KeyPress::Stay);
        assert_eq!(k.press(SetupKey::B), KeyPress::Empty);
    }

    #[test]
    fn start_and_the_last_action_key_are_done() {
        let mut k = Keyboard::new(10);
        assert_eq!(k.press(SetupKey::Start), KeyPress::Done);
        k.press(SetupKey::Up);
        for _ in 0..3 {
            k.press(SetupKey::Right);
        }
        assert_eq!(k.press(SetupKey::A), KeyPress::Done);
    }

    #[test]
    fn a_masked_keyboard_never_prints_what_was_typed() {
        let mut k = Keyboard::masked(10);
        k.press(SetupKey::Down);
        k.press(SetupKey::A);
        assert_eq!(k.typed(), "q");
        assert!(!format!("{k:?}").contains('q'));
        assert_eq!(k.take(), "q");
        assert_eq!(k.typed(), "");
    }
}
