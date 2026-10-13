//! RetroAchievements in the app: who is signed in, kept on the card as a name
//! and a token in `Config/retroachievements.conf`, and what rcheevos says.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use slot_cheevos::{Cheevos, Event, Secret};
use slot_retro::RaConsole;
use slot_store::Platform;
use slot_ui::{Cheevo, Millis};

/// How long after a sign-in that could not reach the server to try again.
pub const RETRY_MS: Millis = 60_000;

/// The client this build talks to: the real one on the device, none on the
/// host, as art.rs and update.rs do.
pub fn cheevos_client(release: &str) -> Arc<dyn Cheevos> {
    #[cfg(feature = "device")]
    {
        let release = match release.trim() {
            "" => env!("CARGO_PKG_VERSION"),
            r => r,
        };
        slot_cheevos::client(&format!("slot/{release}"))
    }
    #[cfg(not(feature = "device"))]
    {
        let _ = release;
        slot_cheevos::no_cheevos()
    }
}

/// The console rcheevos knows a platform's games as: it picks the hash and
/// how the core's memory is laid out. Either core will do; gpSP leaves SRAM
/// out of its maps and the GBA layout reads it from the save RAM instead.
pub fn ra_console(platform: Platform) -> RaConsole {
    match platform {
        Platform::Gba => RaConsole::Gba,
        Platform::Gb => RaConsole::Gb,
        Platform::Gbc => RaConsole::Gbc,
    }
}

/// Hashes a ROM the way rcheevos hashes this console's games.
pub fn rom_hash(console: RaConsole, rom: &Path) -> std::io::Result<String> {
    slot_cheevos::rom_hash(console.rc_id(), rom)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CheevosState {
    Off,
    /// A name and token are on the card, not yet checked this run.
    Saved(String),
    SigningIn(String),
    SignedIn(String),
    Failed,
}

/// What the app shows for something rcheevos said.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Notice {
    SignedIn(String),
    /// `rejected` when the server refused the name or password.
    SignInFailed {
        rejected: bool,
    },
    Popup(Cheevo),
}

pub struct Account {
    api: Arc<dyn Cheevos>,
    root: Option<PathBuf>,
    state: CheevosState,
    saved: Option<(String, Secret)>,
    /// When to try the saved token, once Wi-Fi is up.
    resume_at: Option<Millis>,
}

impl Account {
    pub fn new(api: Arc<dyn Cheevos>) -> Account {
        Account {
            api,
            root: None,
            state: CheevosState::Off,
            saved: None,
            resume_at: None,
        }
    }

    /// Picks up a sign-in left on the card; it is tried once Wi-Fi is up.
    pub fn boot(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        if let Some((username, token)) = slot_store::read_cheevos(root) {
            eprintln!("slot: cheevos: signed in before as {username}");
            self.state = CheevosState::Saved(username.clone());
            self.saved = Some((username, Secret::new(token)));
            self.resume_at = Some(0);
        }
    }

    pub fn api(&self) -> Arc<dyn Cheevos> {
        self.api.clone()
    }

    pub fn set_api(&mut self, api: Arc<dyn Cheevos>) {
        self.api = api;
    }

    pub fn state(&self) -> &CheevosState {
        &self.state
    }

    /// Signed in this run, so a game can be loaded.
    pub fn signed_in(&self) -> bool {
        matches!(self.state, CheevosState::SignedIn(_))
    }

    /// Whether A on the menu row signs out rather than opening the sign-in.
    pub fn has_account(&self) -> bool {
        !matches!(self.state, CheevosState::Off | CheevosState::Failed)
    }

    pub fn text(&self) -> String {
        match &self.state {
            CheevosState::Off => "Off".into(),
            CheevosState::SigningIn(_) => "Signing In...".into(),
            CheevosState::Saved(name) | CheevosState::SignedIn(name) => name.clone(),
            CheevosState::Failed => "Failed".into(),
        }
    }

    pub fn sign_in(&mut self, username: &str, password: String) {
        eprintln!("slot: cheevos: signing in as {username}");
        self.state = CheevosState::SigningIn(username.into());
        self.resume_at = None;
        self.api.sign_in(username, Secret::new(password));
    }

    pub fn sign_out(&mut self) {
        eprintln!("slot: cheevos: signing out");
        self.api.unload_game();
        self.api.sign_out();
        self.forget();
        self.state = CheevosState::Off;
    }

    fn forget(&mut self) {
        self.saved = None;
        self.resume_at = None;
        if let Some(root) = &self.root {
            if let Err(e) = slot_store::forget_cheevos(root) {
                eprintln!("slot: cheevos: could not forget the sign-in: {e}");
            }
        }
    }

    /// Tries the saved token when it is due and the network is up.
    pub fn timers(&mut self, online: bool, now: Millis) {
        let due = self.resume_at.is_some_and(|at| now >= at);
        if !online || !due {
            return;
        }
        let Some((username, token)) = &self.saved else {
            self.resume_at = None;
            return;
        };
        self.resume_at = None;
        self.state = CheevosState::SigningIn(username.clone());
        self.api.resume(username, token);
    }

    /// Takes what rcheevos has said since last time.
    pub fn poll(&mut self, now: Millis) -> Vec<Notice> {
        let mut notices = Vec::new();
        for event in self.api.take_events() {
            match event {
                Event::SignedIn { username, token } => {
                    if let Some(root) = &self.root {
                        if let Err(e) = slot_store::write_cheevos(root, &username, token.expose()) {
                            eprintln!("slot: cheevos: could not save the sign-in: {e}");
                        }
                    }
                    self.saved = Some((username.clone(), token));
                    self.state = CheevosState::SignedIn(username.clone());
                    notices.push(Notice::SignedIn(username));
                }
                Event::SignInFailed { rejected, .. } => {
                    let resuming =
                        matches!(self.state, CheevosState::SigningIn(_)) && self.saved.is_some();
                    if rejected {
                        eprintln!("slot: cheevos: the sign-in was refused, forgetting it");
                        self.forget();
                        self.state = CheevosState::Failed;
                    } else if resuming {
                        self.resume_at = Some(now + RETRY_MS);
                        if let Some((name, _)) = &self.saved {
                            self.state = CheevosState::Saved(name.clone());
                        }
                    } else {
                        self.state = CheevosState::Failed;
                    }
                    notices.push(Notice::SignInFailed { rejected });
                }
                Event::Unlocked {
                    title,
                    description,
                    points,
                } => notices.push(Notice::Popup(Cheevo::unlocked(
                    &title,
                    &description,
                    points,
                ))),
                Event::Completed { title } => {
                    notices.push(Notice::Popup(Cheevo::completed(&title)))
                }
                Event::GameLoaded { .. } | Event::GameFailed { .. } | Event::ServerError { .. } => {
                }
            }
        }
        notices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        events: Mutex<Vec<Event>>,
    }

    impl Cheevos for Fake {
        fn sign_in(&self, username: &str, password: Secret) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("sign_in {username} {}", password.expose()));
        }
        fn resume(&self, username: &str, token: &Secret) {
            self.calls
                .lock()
                .unwrap()
                .push(format!("resume {username} {}", token.expose()));
        }
        fn sign_out(&self) {
            self.calls.lock().unwrap().push("sign_out".into());
        }
        fn signed_in(&self) -> bool {
            false
        }
        fn load_game(&self, _md5: &str) {}
        fn unload_game(&self) {}
        fn set_memory(&self, _regions: Vec<slot_cheevos::Region>) {}
        fn do_frame(&self) {}
        fn idle(&self) {}
        fn reset_progress(&self) {}
        fn take_events(&self) -> Vec<Event> {
            std::mem::take(&mut self.events.lock().unwrap())
        }
    }

    fn account() -> (tempfile::TempDir, Arc<Fake>, Account) {
        let d = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake::default());
        let mut a = Account::new(fake.clone());
        a.boot(d.path());
        (d, fake, a)
    }

    #[test]
    fn a_password_sign_in_saves_the_token_and_not_the_password() {
        let (d, fake, mut a) = account();
        assert_eq!(a.text(), "Off");
        a.sign_in("me", "hunter2".into());
        assert_eq!(a.text(), "Signing In...");
        fake.events.lock().unwrap().push(Event::SignedIn {
            username: "me".into(),
            token: Secret::new("tok".into()),
        });
        assert_eq!(a.poll(0), [Notice::SignedIn("me".into())]);
        assert!(a.signed_in());
        assert_eq!(a.text(), "me");
        assert_eq!(
            slot_store::read_cheevos(d.path()),
            Some(("me".into(), "tok".into()))
        );
        let conf = std::fs::read_to_string(d.path().join(slot_store::CHEEVOS_FILE)).unwrap();
        assert!(!conf.contains("hunter2"));
    }

    #[test]
    fn a_saved_token_is_tried_once_wifi_is_up() {
        let d = tempfile::tempdir().unwrap();
        slot_store::write_cheevos(d.path(), "me", "tok").unwrap();
        let fake = Arc::new(Fake::default());
        let mut a = Account::new(fake.clone());
        a.boot(d.path());
        assert_eq!(a.text(), "me");
        assert!(!a.signed_in());
        a.timers(false, 10);
        assert!(fake.calls.lock().unwrap().is_empty());
        a.timers(true, 20);
        a.timers(true, 30);
        assert_eq!(*fake.calls.lock().unwrap(), ["resume me tok"]);
    }

    #[test]
    fn a_token_that_cannot_reach_the_server_is_tried_again_later() {
        let d = tempfile::tempdir().unwrap();
        slot_store::write_cheevos(d.path(), "me", "tok").unwrap();
        let fake = Arc::new(Fake::default());
        let mut a = Account::new(fake.clone());
        a.boot(d.path());
        a.timers(true, 0);
        fake.events.lock().unwrap().push(Event::SignInFailed {
            message: String::new(),
            rejected: false,
        });
        a.poll(100);
        assert_eq!(a.text(), "me");
        a.timers(true, 100 + RETRY_MS - 1);
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
        a.timers(true, 100 + RETRY_MS);
        assert_eq!(fake.calls.lock().unwrap().len(), 2);
        assert!(slot_store::read_cheevos(d.path()).is_some());
    }

    #[test]
    fn a_rejected_token_is_forgotten() {
        let d = tempfile::tempdir().unwrap();
        slot_store::write_cheevos(d.path(), "me", "tok").unwrap();
        let fake = Arc::new(Fake::default());
        let mut a = Account::new(fake.clone());
        a.boot(d.path());
        a.timers(true, 0);
        fake.events.lock().unwrap().push(Event::SignInFailed {
            message: String::new(),
            rejected: true,
        });
        assert_eq!(a.poll(0), [Notice::SignInFailed { rejected: true }]);
        assert_eq!(a.text(), "Failed");
        assert_eq!(slot_store::read_cheevos(d.path()), None);
        a.timers(true, RETRY_MS * 10);
        assert_eq!(fake.calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn signing_out_forgets_the_card_and_tells_rcheevos() {
        let d = tempfile::tempdir().unwrap();
        slot_store::write_cheevos(d.path(), "me", "tok").unwrap();
        let fake = Arc::new(Fake::default());
        let mut a = Account::new(fake.clone());
        a.boot(d.path());
        assert!(a.has_account());
        a.sign_out();
        assert!(!a.has_account());
        assert_eq!(a.text(), "Off");
        assert_eq!(slot_store::read_cheevos(d.path()), None);
        assert_eq!(*fake.calls.lock().unwrap(), ["sign_out"]);
    }

    #[test]
    fn unlocks_become_popups() {
        let (_d, fake, mut a) = account();
        fake.events.lock().unwrap().extend([
            Event::Unlocked {
                title: "Hello".into(),
                description: "Say hi".into(),
                points: 5,
            },
            Event::Completed {
                title: "Game".into(),
            },
        ]);
        assert_eq!(
            a.poll(0),
            [
                Notice::Popup(Cheevo::unlocked("Hello", "Say hi", 5)),
                Notice::Popup(Cheevo::completed("Game")),
            ]
        );
    }
}
