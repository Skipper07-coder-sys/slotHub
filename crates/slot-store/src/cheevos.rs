//! The RetroAchievements sign-in: the name and the token the server handed
//! back. The password is never stored.

use std::path::Path;

use crate::ini;

pub const CHEEVOS_FILE: &str = "Config/retroachievements.conf";

const USERNAME_KEY: &str = "username";
const TOKEN_KEY: &str = "token";

/// The saved name and token, when both are there.
pub fn read_cheevos(root: &Path) -> Option<(String, String)> {
    let mut entries = ini::read(root, CHEEVOS_FILE);
    let username = entries.remove(USERNAME_KEY).filter(|v| !v.is_empty())?;
    let token = entries.remove(TOKEN_KEY).filter(|v| !v.is_empty())?;
    Some((username, token))
}

pub fn write_cheevos(root: &Path, username: &str, token: &str) -> std::io::Result<()> {
    let text = ini::set("", USERNAME_KEY, Some(username))?;
    let text = ini::set(&text, TOKEN_KEY, Some(token))?;
    let path = root.join(CHEEVOS_FILE);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::atomic::atomic_write(&path, text.as_bytes())
}

/// Signs out for good: the token is gone from the card.
pub fn forget_cheevos(root: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(root.join(CHEEVOS_FILE)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_is_written_read_back_and_forgotten() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read_cheevos(d.path()), None);
        write_cheevos(d.path(), "Player One", "AbC123xyz").unwrap();
        assert_eq!(
            read_cheevos(d.path()),
            Some(("Player One".into(), "AbC123xyz".into()))
        );
        let text = std::fs::read_to_string(d.path().join(CHEEVOS_FILE)).unwrap();
        assert_eq!(text, "username = Player One\ntoken = AbC123xyz\n");
        write_cheevos(d.path(), "p2", "t2").unwrap();
        assert_eq!(read_cheevos(d.path()), Some(("p2".into(), "t2".into())));
        forget_cheevos(d.path()).unwrap();
        assert_eq!(read_cheevos(d.path()), None);
        forget_cheevos(d.path()).unwrap();
    }

    #[test]
    fn a_file_missing_the_token_is_no_sign_in() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("Config")).unwrap();
        std::fs::write(d.path().join(CHEEVOS_FILE), "username = me\ntoken =\n").unwrap();
        assert_eq!(read_cheevos(d.path()), None);
    }

    #[test]
    fn a_name_that_would_break_the_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(write_cheevos(d.path(), "me\ntoken = x", "t").is_err());
        assert_eq!(read_cheevos(d.path()), None);
    }
}
