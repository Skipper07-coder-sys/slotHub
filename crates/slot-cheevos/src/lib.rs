//! RetroAchievements for slot, through the vendored rcheevos client.
//!
//! The app talks to a [`Cheevos`]: the real one wraps one process-wide
//! `rc_client`, and [`NoCheevos`] stands in where there is no network, as in
//! tests. Softcore only: hardcore is always off.

mod client;
mod ffi;
pub mod net;

use std::io::Read;
use std::path::Path;
use std::sync::Arc;

pub use client::{client, redact};

/// Text that is overwritten when dropped and never printed: a password or a
/// token.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: String) -> Secret {
        Secret(text)
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        net::wipe(&mut bytes);
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[secret]")
    }
}

/// What rcheevos has to tell the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    SignedIn {
        username: String,
        token: Secret,
    },
    /// `rejected` when the server turned the name, password or token down,
    /// rather than not being reached.
    SignInFailed {
        message: String,
        rejected: bool,
    },
    GameLoaded {
        title: String,
        achievements: u32,
        unlocked: u32,
    },
    GameFailed {
        message: String,
    },
    Unlocked {
        title: String,
        description: String,
        points: u32,
    },
    Completed {
        title: String,
    },
    ServerError {
        message: String,
    },
}

/// A run of RetroAchievements addresses backed by core memory.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u32,
    pub ptr: *const u8,
    pub len: usize,
}

pub trait Cheevos: Send + Sync {
    /// Signs in with a password. The password is wiped once it is sent.
    fn sign_in(&self, username: &str, password: Secret);
    /// Signs in again with the token from an earlier sign-in.
    fn resume(&self, username: &str, token: &Secret);
    fn sign_out(&self);
    /// Whether the server has accepted a sign-in this run.
    fn signed_in(&self) -> bool;
    /// Starts identifying the game whose ROM has this MD5, as lowercase hex.
    fn load_game(&self, md5: &str);
    fn unload_game(&self);
    /// The memory the achievements read, for the calling thread: call it on
    /// the thread that calls `do_frame`.
    fn set_memory(&self, regions: Vec<Region>);
    fn do_frame(&self);
    fn idle(&self);
    /// Forgets how far achievements had got, after a state load or rewind.
    fn reset_progress(&self);
    fn take_events(&self) -> Vec<Event>;
}

/// No RetroAchievements at all.
pub struct NoCheevos;

impl Cheevos for NoCheevos {
    fn sign_in(&self, _username: &str, _password: Secret) {}
    fn resume(&self, _username: &str, _token: &Secret) {}
    fn sign_out(&self) {}
    fn signed_in(&self) -> bool {
        false
    }
    fn load_game(&self, _md5: &str) {}
    fn unload_game(&self) {}
    fn set_memory(&self, _regions: Vec<Region>) {}
    fn do_frame(&self) {}
    fn idle(&self) {}
    fn reset_progress(&self) {}
    fn take_events(&self) -> Vec<Event> {
        Vec::new()
    }
}

pub fn no_cheevos() -> Arc<dyn Cheevos> {
    Arc::new(NoCheevos)
}

/// MD5 of some bytes as lowercase hex, through rcheevos's own MD5.
pub fn md5_hex(bytes: &[u8]) -> String {
    let mut md5 = Md5::new();
    md5.append(bytes);
    md5.hex()
}

/// The hash RetroAchievements knows a GBA ROM by: the MD5 of the whole file.
pub fn rom_md5(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut md5 = Md5::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        md5.append(&buf[..n]);
    }
    Ok(md5.hex())
}

struct Md5(ffi::md5_state_t);

impl Md5 {
    fn new() -> Md5 {
        let mut state = ffi::md5_state_t {
            count: [0; 2],
            abcd: [0; 4],
            buf: [0; 64],
        };
        unsafe { ffi::md5_init(&mut state) };
        Md5(state)
    }

    fn append(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(i32::MAX as usize) {
            unsafe { ffi::md5_append(&mut self.0, chunk.as_ptr(), chunk.len() as i32) };
        }
    }

    fn hex(mut self) -> String {
        let mut digest = [0u8; 16];
        unsafe { ffi::md5_finish(&mut self.0, digest.as_mut_ptr()) };
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::Mutex;

    static CURL_ENV: Mutex<()> = Mutex::new(());

    /// Runs `f` with SLOT_CURL pointing at a shell script made of `script`.
    pub(crate) fn with_curl<R>(dir: &std::path::Path, script: &str, f: impl FnOnce() -> R) -> R {
        use std::os::unix::fs::PermissionsExt;
        let _held = CURL_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let curl = dir.join("curl");
        std::fs::write(&curl, format!("#!/bin/sh\n{script}")).unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("SLOT_CURL", &curl);
        let r = f();
        std::env::remove_var("SLOT_CURL");
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_matches_the_rfc_test_suite() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(
            md5_hex(
                b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
            ),
            "57edf4a22be3c955ac49da2e2107b67a"
        );
    }

    #[test]
    fn a_rom_is_hashed_whole_however_it_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let rom = dir.path().join("game.gba");
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i * 7) as u8).collect();
        std::fs::write(&rom, &bytes).unwrap();
        assert_eq!(rom_md5(&rom).unwrap(), md5_hex(&bytes));
    }

    #[test]
    fn a_secret_never_prints() {
        let s = Secret::new("hunter2".into());
        assert_eq!(format!("{s:?}"), "[secret]");
        let e = Event::SignedIn {
            username: "me".into(),
            token: Secret::new("abc123".into()),
        };
        assert!(!format!("{e:?}").contains("abc123"));
    }
}
