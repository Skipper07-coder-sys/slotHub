use slot_gfx::{Draw, TexId, OUT_W};

use crate::plate::UndoFace;
use crate::wifi_setup::{
    bars, Canvas, SetupKey, WifiSetup, BODY_PX, DIM, EDGE, INK, LIT, LIT_INK, PAPER, ROWS_SHOWN,
    ROW_GAP, ROW_H, ROW_Y, TITLE_H, TITLE_PX, TITLE_Y,
};

const TITLE: &str = "Bluetooth Headphones";
const PAIRING_HINT: &str = "Put them in pairing mode first";
const NOTE_Y: u32 = 180;
const NOTE_GAP: u32 = 50;
const NOTE_H: u32 = 48;
// bluetooth.sh prints -999 for headphones BlueZ remembers but did not hear.
const UNHEARD: i32 = -200;

/// Headphones the card's scan found, as `bluetooth.sh scan` lists them.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BtFound {
    pub mac: String,
    pub name: String,
    pub paired: bool,
    pub signal: i32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BtSetup {
    Scanning,
    List {
        devices: Vec<BtFound>,
        cursor: usize,
    },
    Connecting {
        name: String,
    },
    Refused {
        name: String,
    },
    Failed {
        name: String,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BtOutcome {
    Stay,
    Rescan,
    Connect { mac: String, name: String },
    Back,
}

impl BtSetup {
    pub fn scanned(devices: Vec<BtFound>) -> BtSetup {
        BtSetup::List { devices, cursor: 0 }
    }

    pub fn press(&mut self, key: SetupKey) -> BtOutcome {
        match self {
            BtSetup::Scanning | BtSetup::Connecting { .. } => match key {
                SetupKey::B => BtOutcome::Back,
                _ => BtOutcome::Stay,
            },
            BtSetup::Refused { .. } | BtSetup::Failed { .. } => match key {
                SetupKey::B => BtOutcome::Back,
                SetupKey::A | SetupKey::X => BtOutcome::Rescan,
                _ => BtOutcome::Stay,
            },
            BtSetup::List { devices, cursor } => {
                let n = devices.len();
                match key {
                    SetupKey::Up if n > 0 => *cursor = (*cursor + n - 1) % n,
                    SetupKey::Down if n > 0 => *cursor = (*cursor + 1) % n,
                    SetupKey::X => return BtOutcome::Rescan,
                    SetupKey::B => return BtOutcome::Back,
                    SetupKey::A => {
                        return match devices.get(*cursor) {
                            Some(d) => BtOutcome::Connect {
                                mac: d.mac.clone(),
                                name: shown_name(d),
                            },
                            None => BtOutcome::Rescan,
                        }
                    }
                    _ => {}
                }
                BtOutcome::Stay
            }
        }
    }

    pub fn face(&self) -> UndoFace {
        let mut canvas = Canvas::new();
        canvas.text_centred(TITLE, TITLE_Y, TITLE_H, TITLE_PX, INK);
        match self {
            BtSetup::Scanning => {
                note(&mut canvas, &["Looking for headphones...", PAIRING_HINT]);
                canvas.hints(&[("B", "Back")]);
            }
            BtSetup::List { devices, cursor } => {
                if devices.is_empty() {
                    note(&mut canvas, &["No headphones found", PAIRING_HINT]);
                }
                let first = cursor.saturating_sub(ROWS_SHOWN - 1);
                for (i, d) in devices.iter().enumerate().skip(first).take(ROWS_SHOWN) {
                    let y = ROW_Y + (i - first) as u32 * (ROW_H + ROW_GAP);
                    let lit = i == *cursor;
                    let (bg, ink) = if lit { (LIT, LIT_INK) } else { (PAPER, INK) };
                    canvas.fill(EDGE, y, OUT_W - 2 * EDGE, ROW_H, bg);
                    let heard = if d.signal > UNHEARD {
                        bars(d.signal)
                    } else {
                        ""
                    };
                    let right = match d.paired {
                        true => format!("{heard} Paired").trim().to_string(),
                        false => heard.to_string(),
                    };
                    let rw = canvas.text_right(&right, OUT_W - EDGE - 14, y, ROW_H, BODY_PX, ink);
                    let room = OUT_W - 2 * EDGE - 28 - rw - 16;
                    canvas.text_left(&shown_name(d), EDGE + 14, y, room, ROW_H, ink);
                }
                canvas.hints(&[("B", "Back"), ("X", "Search Again"), ("A", "Connect")]);
            }
            BtSetup::Connecting { name } => {
                note(&mut canvas, &[&format!("Connecting to {name}...")]);
                canvas.hints(&[("B", "Back")]);
            }
            BtSetup::Refused { name } => {
                note(
                    &mut canvas,
                    &[
                        &format!("{name} refused to pair"),
                        "Put them in pairing mode and try again",
                    ],
                );
                canvas.hints(&[("B", "Back"), ("A", "Search Again")]);
            }
            BtSetup::Failed { name } => {
                note(
                    &mut canvas,
                    &[
                        &format!("Could not connect to {name}"),
                        "Check they are on and nearby, then try again",
                    ],
                );
                canvas.hints(&[("B", "Back"), ("A", "Search Again")]);
            }
        }
        canvas.face()
    }

    pub fn draw(face: Option<TexId>, out: &mut Vec<Draw>) {
        WifiSetup::draw(face, out);
    }
}

fn shown_name(d: &BtFound) -> String {
    match d.name.is_empty() {
        true => d.mac.clone(),
        false => d.name.clone(),
    }
}

fn note(canvas: &mut Canvas, lines: &[&str]) {
    for (i, line) in lines.iter().enumerate() {
        canvas.text_centred(line, NOTE_Y + i as u32 * NOTE_GAP, NOTE_H, BODY_PX, DIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found() -> Vec<BtFound> {
        vec![
            BtFound {
                mac: "30:7A:D2:85:D8:1D".into(),
                name: "Le Battonetes 2".into(),
                paired: true,
                signal: -40,
            },
            BtFound {
                mac: "AA:AA:AA:AA:AA:02".into(),
                name: String::new(),
                paired: false,
                signal: -80,
            },
        ]
    }

    #[test]
    fn a_connects_to_the_headphones_under_the_cursor_and_wraps_around() {
        let mut s = BtSetup::scanned(found());
        assert_eq!(s.press(SetupKey::Down), BtOutcome::Stay);
        assert_eq!(
            s.press(SetupKey::A),
            BtOutcome::Connect {
                mac: "AA:AA:AA:AA:AA:02".into(),
                name: "AA:AA:AA:AA:AA:02".into()
            }
        );
        assert_eq!(s.press(SetupKey::Down), BtOutcome::Stay);
        assert_eq!(
            s.press(SetupKey::A),
            BtOutcome::Connect {
                mac: "30:7A:D2:85:D8:1D".into(),
                name: "Le Battonetes 2".into()
            }
        );
        assert_eq!(s.press(SetupKey::X), BtOutcome::Rescan);
        assert_eq!(s.press(SetupKey::B), BtOutcome::Back);
    }

    #[test]
    fn an_empty_list_searches_again_on_a_and_a_refusal_does_too() {
        let mut s = BtSetup::scanned(Vec::new());
        assert_eq!(s.press(SetupKey::Up), BtOutcome::Stay);
        assert_eq!(s.press(SetupKey::A), BtOutcome::Rescan);
        let mut s = BtSetup::Refused {
            name: "Le Battonetes 2".into(),
        };
        assert_eq!(s.press(SetupKey::Down), BtOutcome::Stay);
        assert_eq!(s.press(SetupKey::A), BtOutcome::Rescan);
        assert_eq!(s.press(SetupKey::B), BtOutcome::Back);
        let mut s = BtSetup::Scanning;
        assert_eq!(s.press(SetupKey::A), BtOutcome::Stay);
        assert_eq!(s.press(SetupKey::B), BtOutcome::Back);
    }

    #[test]
    fn every_screen_draws_a_full_face() {
        for s in [
            BtSetup::Scanning,
            BtSetup::scanned(found()),
            BtSetup::scanned(Vec::new()),
            BtSetup::Connecting { name: "x".into() },
            BtSetup::Refused { name: "x".into() },
            BtSetup::Failed { name: "x".into() },
        ] {
            let f = s.face();
            assert_eq!(f.rgba.len(), (f.w * f.h * 4) as usize);
        }
    }
}
