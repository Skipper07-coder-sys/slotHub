//! Signing in to RetroAchievements: a name, a password, then the answer.

use slot_gfx::Draw;

use crate::keyboard::{KeyPress, Keyboard};
use crate::plate::UndoFace;
use crate::wifi_setup::{
    Canvas, SetupKey, WifiSetup, BODY_PX, DIM, INK, TITLE_H, TITLE_PX, TITLE_Y,
};

pub const CHEEVOS_NAME_MAX: usize = 32;
pub const CHEEVOS_PASSWORD_MAX: usize = 64;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CheevosSetup {
    Username { keys: Keyboard },
    Password { username: String, keys: Keyboard },
    SigningIn { username: String },
    SignedIn { username: String },
    Failed { username: String, message: String },
}

#[derive(Clone, PartialEq, Eq)]
pub enum CheevosOutcome {
    Stay,
    Back,
    SignIn { username: String, password: String },
}

impl std::fmt::Debug for CheevosOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheevosOutcome::Stay => f.write_str("Stay"),
            CheevosOutcome::Back => f.write_str("Back"),
            CheevosOutcome::SignIn { username, .. } => write!(f, "SignIn({username:?})"),
        }
    }
}

impl Default for CheevosSetup {
    fn default() -> Self {
        CheevosSetup::Username {
            keys: Keyboard::new(CHEEVOS_NAME_MAX),
        }
    }
}

impl CheevosSetup {
    pub fn press(&mut self, key: SetupKey) -> CheevosOutcome {
        match self {
            CheevosSetup::Username { keys } => match keys.press(key) {
                KeyPress::Stay => CheevosOutcome::Stay,
                KeyPress::Empty => CheevosOutcome::Back,
                KeyPress::Done => {
                    let username = keys.typed().trim().to_string();
                    if !username.is_empty() {
                        *self = CheevosSetup::Password {
                            username,
                            keys: Keyboard::masked(CHEEVOS_PASSWORD_MAX),
                        };
                    }
                    CheevosOutcome::Stay
                }
            },
            CheevosSetup::Password { username, keys } => match keys.press(key) {
                KeyPress::Stay => CheevosOutcome::Stay,
                KeyPress::Empty => {
                    *self = CheevosSetup::Username {
                        keys: Keyboard::with(CHEEVOS_NAME_MAX, username),
                    };
                    CheevosOutcome::Stay
                }
                KeyPress::Done if keys.typed().is_empty() => CheevosOutcome::Stay,
                KeyPress::Done => {
                    let password = keys.take();
                    let username = std::mem::take(username);
                    *self = CheevosSetup::SigningIn {
                        username: username.clone(),
                    };
                    CheevosOutcome::SignIn { username, password }
                }
            },
            CheevosSetup::SigningIn { .. } | CheevosSetup::SignedIn { .. } => match key {
                SetupKey::A | SetupKey::B | SetupKey::Start => CheevosOutcome::Back,
                _ => CheevosOutcome::Stay,
            },
            CheevosSetup::Failed { username, .. } => match key {
                SetupKey::A | SetupKey::Start => {
                    *self = CheevosSetup::Password {
                        username: std::mem::take(username),
                        keys: Keyboard::masked(CHEEVOS_PASSWORD_MAX),
                    };
                    CheevosOutcome::Stay
                }
                SetupKey::B => CheevosOutcome::Back,
                _ => CheevosOutcome::Stay,
            },
        }
    }

    pub fn face(&self) -> UndoFace {
        let mut canvas = Canvas::new();
        match self {
            CheevosSetup::Username { keys } => {
                canvas.text_centred("RetroAchievements Name", TITLE_Y, TITLE_H, TITLE_PX, INK);
                keys.draw(&mut canvas, "Next");
                canvas.hints(&[
                    ("B", "Delete"),
                    ("X", "ABC #+="),
                    ("Y", "Space"),
                    ("START", "Next"),
                ]);
            }
            CheevosSetup::Password { username, keys } => {
                let title = format!("Password for {username}");
                canvas.text_centred(&title, TITLE_Y, TITLE_H, TITLE_PX, INK);
                keys.draw(&mut canvas, "Sign In");
                canvas.hints(&[
                    ("B", "Delete"),
                    ("X", "ABC #+="),
                    ("Y", "Space"),
                    ("START", "Sign In"),
                ]);
            }
            CheevosSetup::SigningIn { username } => {
                canvas.text_centred("RetroAchievements", TITLE_Y, TITLE_H, TITLE_PX, INK);
                let line = format!("Signing in as {username}...");
                canvas.text_centred(&line, 200, 48, BODY_PX, DIM);
                canvas.hints(&[("B", "Back")]);
            }
            CheevosSetup::SignedIn { username } => {
                canvas.text_centred("RetroAchievements", TITLE_Y, TITLE_H, TITLE_PX, INK);
                let line = format!("Signed in as {username}");
                canvas.text_centred(&line, 180, 48, BODY_PX, INK);
                canvas.text_centred("GBA games now unlock achievements", 230, 48, BODY_PX, DIM);
                canvas.hints(&[("A", "Done")]);
            }
            CheevosSetup::Failed { message, .. } => {
                canvas.text_centred("RetroAchievements", TITLE_Y, TITLE_H, TITLE_PX, INK);
                canvas.text_centred("Could not sign in", 180, 48, BODY_PX, INK);
                canvas.text_centred(message, 230, 48, BODY_PX, DIM);
                canvas.hints(&[("B", "Back"), ("A", "Try Again")]);
            }
        }
        canvas.face()
    }

    pub fn draw(face: Option<slot_gfx::TexId>, out: &mut Vec<Draw>) {
        WifiSetup::draw(face, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slot_gfx::{OUT_H, OUT_W};

    fn type_first_keys(s: &mut CheevosSetup, n: usize) {
        // The second row of the first page: q w e r t...
        s.press(SetupKey::Down);
        for _ in 0..n {
            s.press(SetupKey::A);
            s.press(SetupKey::Right);
        }
    }

    #[test]
    fn a_name_then_a_password_signs_in() {
        let mut s = CheevosSetup::default();
        assert_eq!(s.press(SetupKey::Start), CheevosOutcome::Stay);
        assert!(
            matches!(s, CheevosSetup::Username { .. }),
            "an empty name went on"
        );
        type_first_keys(&mut s, 2);
        s.press(SetupKey::Start);
        assert!(matches!(&s, CheevosSetup::Password { username, .. } if username == "qw"));
        assert_eq!(s.press(SetupKey::Start), CheevosOutcome::Stay);
        type_first_keys(&mut s, 3);
        assert_eq!(
            s.press(SetupKey::Start),
            CheevosOutcome::SignIn {
                username: "qw".into(),
                password: "qwe".into()
            }
        );
        assert_eq!(
            s,
            CheevosSetup::SigningIn {
                username: "qw".into()
            }
        );
    }

    #[test]
    fn b_on_an_empty_password_goes_back_to_the_name_as_typed() {
        let mut s = CheevosSetup::Password {
            username: "me".into(),
            keys: Keyboard::masked(CHEEVOS_PASSWORD_MAX),
        };
        s.press(SetupKey::B);
        assert!(matches!(&s, CheevosSetup::Username { keys } if keys.typed() == "me"));
        s.press(SetupKey::B);
        s.press(SetupKey::B);
        assert_eq!(s.press(SetupKey::B), CheevosOutcome::Back);
    }

    #[test]
    fn a_failed_sign_in_tries_again_with_a_fresh_password() {
        let mut s = CheevosSetup::Failed {
            username: "me".into(),
            message: "No connection".into(),
        };
        s.press(SetupKey::A);
        assert!(
            matches!(&s, CheevosSetup::Password { username, keys } if username == "me" && keys.typed().is_empty())
        );
        let mut s = CheevosSetup::Failed {
            username: "me".into(),
            message: String::new(),
        };
        assert_eq!(s.press(SetupKey::B), CheevosOutcome::Back);
    }

    #[test]
    fn the_password_never_shows_in_debug_output() {
        let mut s = CheevosSetup::Password {
            username: "me".into(),
            keys: Keyboard::masked(CHEEVOS_PASSWORD_MAX),
        };
        type_first_keys(&mut s, 4);
        assert!(!format!("{s:?}").contains("qwer"));
        let out = s.press(SetupKey::Start);
        assert!(!format!("{out:?}").contains("qwer"));
    }

    #[test]
    fn every_screen_draws_a_full_frame() {
        for s in [
            CheevosSetup::default(),
            CheevosSetup::Password {
                username: "me".into(),
                keys: Keyboard::masked(CHEEVOS_PASSWORD_MAX),
            },
            CheevosSetup::SigningIn {
                username: "me".into(),
            },
            CheevosSetup::SignedIn {
                username: "me".into(),
            },
            CheevosSetup::Failed {
                username: "me".into(),
                message: "Invalid User/Password combination. Please try again".into(),
            },
        ] {
            let f = s.face();
            assert_eq!((f.w, f.h), (OUT_W, OUT_H));
            assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "{s:?} drew nothing");
        }
    }
}
