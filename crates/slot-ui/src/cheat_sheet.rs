//! Button Cheat Sheet: every shortcut on a few pages, opened with A from
//! About. Ported from marghost/slot_modded (GPL-3.0), commit defe26b, and
//! fitted to slotHub's controls and pure mode.

use crate::draw::{Draw, TexId, OUT_H, OUT_W};
use crate::plate::{arrows_hint_face, centred_hints, hint_face, UndoFace, HINT_H, LEGEND_GAP};
use crate::power_menu::MENU_INK;
use crate::quick_menu::{DIM_INK, LEGEND_Y, QUICK_EDGE};
use crate::slot_chrome::{edge, opening};
use crate::text::{blend, label_font, line_width};

/// One shortcut: its buttons, what it does, and whether pure mode keeps it.
type Row = (&'static str, &'static str, bool);

const PAGES: [(&str, &[Row]); 5] = [
    (
        "SHELF",
        &[
            ("Left / Right", "Browse games", true),
            ("Up / Down", "Jump to next letter", true),
            ("L1 / R1", "Switch platform", true),
            ("A", "Play game", true),
            ("START", "Pick emulator (GBA)", true),
            ("MENU", "Settings", true),
        ],
    ),
    (
        "IN GAME",
        &[
            ("Hold MENU", "Save, back to shelf", true),
            ("SELECT+MENU", "Link menu", true),
            ("MENU x2", "Save state switcher", false),
            ("SELECT+R1", "Save state", false),
            ("SELECT+L1", "Load latest state", false),
            ("Hold L2", "Rewind", false),
            ("Hold R2", "Fast-forward", false),
            ("R2 x2", "Lock fast-forward", false),
            ("Hold X / Y", "Turbo A / Turbo B", false),
        ],
    ),
    (
        "SCREEN & SOUND",
        &[
            ("SEL+Up/Down", "Brightness", true),
            ("SEL+Left/Right", "Blue light", true),
            ("SEL+B / A", "Prev / next shader", true),
            ("SEL+Y", "Color correction", true),
            ("SEL+L2 / R2", "GB palette (if on)", true),
            ("L1 / R1", "GB stretch / 1:1", true),
            ("VOL+ / VOL-", "Volume", true),
            ("SEL+VOL+/-", "Mute", true),
        ],
    ),
    (
        "POWER & LID",
        &[
            ("Tap POWER", "Sleep / wake", true),
            ("Hold POWER", "Power menu", true),
            ("Close lid", "Pause, screen off", true),
        ],
    ),
    (
        "SAVE STATES",
        &[
            ("Left / Right", "Browse states", false),
            ("A", "Load this state", false),
            ("Y", "Delete this state", false),
            ("X", "Undo save / load", false),
            ("B", "Back to game", false),
        ],
    ),
];

pub struct CheatPage {
    pub title: &'static str,
    pub rows: Vec<(&'static str, &'static str)>,
}

/// The pages this handheld shows: pure mode drops the shortcuts it turns
/// off, and a page left with none.
pub fn cheat_pages(pure: bool) -> Vec<CheatPage> {
    PAGES
        .iter()
        .map(|(title, rows)| CheatPage {
            title,
            rows: rows
                .iter()
                .filter(|(_, _, kept)| *kept || !pure)
                .map(|&(key, what, _)| (key, what))
                .collect(),
        })
        .filter(|p| !p.rows.is_empty())
        .collect()
}

pub const CHEAT_HEADER_H: f32 = 56.0;
pub const CHEAT_DESC_X: f32 = 330.0;
pub const CHEAT_TEXT_PX: f32 = 26.0;
const TITLE_PX: f32 = 30.0;
const KEY_INK: [u8; 3] = [250, 200, 80];
const FIRST_ROW: f32 = 90.0;
const PITCH: f32 = 37.0;

/// B Back, the arrows turning pages, and A opening the sheet from About.
pub fn cheat_legend_faces() -> [UndoFace; 3] {
    [
        hint_face("B", "Back"),
        arrows_hint_face("Page"),
        hint_face("A", "Button Help"),
    ]
}

/// The words of one page on a clear face, laid over the menu's ground.
pub fn cheat_sheet_face(page: usize, pure: bool) -> UndoFace {
    let (w, h) = (OUT_W, OUT_H);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let pages = cheat_pages(pure);
    let at = page % pages.len();
    let sheet = &pages[at];

    let header_mid = CHEAT_HEADER_H / 2.0;
    text(
        &mut rgba,
        QUICK_EDGE,
        header_mid,
        sheet.title,
        TITLE_PX,
        MENU_INK,
    );
    let number = format!("{}/{}", at + 1, pages.len());
    let right = w as f32 - QUICK_EDGE - cheat_width(&number, CHEAT_TEXT_PX);
    text(
        &mut rgba,
        right,
        header_mid,
        &number,
        CHEAT_TEXT_PX,
        DIM_INK,
    );

    let mut mid = FIRST_ROW;
    for (key, what) in &sheet.rows {
        text(&mut rgba, QUICK_EDGE, mid, key, CHEAT_TEXT_PX, KEY_INK);
        text(&mut rgba, CHEAT_DESC_X, mid, what, CHEAT_TEXT_PX, MENU_INK);
        mid += PITCH;
    }
    UndoFace { rgba, w, h }
}

/// The lowest row's middle on the longest page, which must stay clear of
/// the legend.
pub fn cheat_last_row_mid(pure: bool) -> f32 {
    let most = cheat_pages(pure)
        .iter()
        .map(|p| p.rows.len())
        .max()
        .unwrap_or(0);
    FIRST_ROW + PITCH * most.saturating_sub(1) as f32
}

/// The menu's ground and title bar, the page's words, then the legend.
pub fn draw_cheat_sheet(face: Option<TexId>, legend: &[(TexId, u32)], out: &mut Vec<Draw>) {
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        colour: opening(),
    });
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: CHEAT_HEADER_H,
        colour: edge(),
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
    draw_legend(legend, out);
}

/// A row of hints along the bottom, where the settings menu keeps its own.
pub fn draw_legend(legend: &[(TexId, u32)], out: &mut Vec<Draw>) {
    for (tex, w, x) in centred_hints(legend, LEGEND_GAP) {
        out.push(Draw::Tex {
            x,
            y: LEGEND_Y,
            w: w as f32,
            h: HINT_H as f32,
            tex,
            alpha: 1.0,
        });
    }
}

pub fn cheat_width(line: &str, px: f32) -> f32 {
    label_font().map_or(0.0, |font| line_width(font, line, px, 0.0))
}

fn text(rgba: &mut [u8], x: f32, mid: f32, line: &str, px: f32, colour: [u8; 3]) {
    let Some(font) = label_font() else { return };
    let cap = font.metrics('H', px).height as f32;
    let baseline = (mid + cap / 2.0).round();
    let (w, h) = (OUT_W as i32, OUT_H as i32);
    let mut pen = x;
    for ch in line.chars() {
        let (m, glyph) = font.rasterize(ch, px);
        let gx = (pen + m.xmin as f32).round() as i32;
        let gy = (baseline - (m.height as f32 + m.ymin as f32)).round() as i32;
        for row in 0..m.height as i32 {
            for col in 0..m.width as i32 {
                let a = glyph[(row * m.width as i32 + col) as usize];
                let (dx, dy) = (gx + col, gy + row);
                if a == 0 || dx < 0 || dy < 0 || dx >= w || dy >= h {
                    continue;
                }
                let i = ((dy * w + dx) * 4) as usize;
                blend(&mut rgba[i..i + 4], a as u32, colour);
            }
        }
        pen += m.advance_width;
    }
}
