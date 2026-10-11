use slot_store::{read_pure, Core, Platform, StateRing, PURE_FILE};
use tempfile::tempdir;

fn root_with(text: Option<&str>) -> tempfile::TempDir {
    let d = tempdir().unwrap();
    std::fs::create_dir(d.path().join("Config")).unwrap();
    if let Some(text) = text {
        std::fs::write(d.path().join(PURE_FILE), text).unwrap();
    }
    d
}

#[test]
fn pure_is_off_until_the_card_asks_for_it() {
    let d = root_with(None);
    assert!(!read_pure(d.path()));
}

#[test]
fn one_line_in_the_config_file_turns_pure_on() {
    for text in [
        "pure = on\n",
        "pure=on",
        "pure = ON\r\n",
        "pure = 1\n",
        "pure = true\n",
        "pure = yes\n",
        "# slotHub\n\n  pure   =   on  \n",
        "\u{feff}pure = on\r\n",
    ] {
        let d = root_with(Some(text));
        assert!(read_pure(d.path()), "{text:?} left pure off");
    }
}

#[test]
fn anything_short_of_a_plain_yes_leaves_pure_off() {
    for text in [
        "",
        "pure\n",
        "on\n",
        "pure = off\n",
        "pure = 0\n",
        "pure = maybe\n",
        "# pure = on\n",
        "; pure = on\n",
        "Pure = on\n",
        "purely = on\n",
    ] {
        let d = root_with(Some(text));
        assert!(!read_pure(d.path()), "{text:?} turned pure on");
    }
}

#[test]
fn the_last_word_in_the_file_wins() {
    let d = root_with(Some("pure = on\npure = off\n"));
    assert!(!read_pure(d.path()));
    let d = root_with(Some("pure = off\npure = on\n"));
    assert!(read_pure(d.path()));
}

#[test]
fn spending_the_resume_removes_it_and_nothing_else() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Emerald");
    r.push(&[1u8; 64], b"png", "2026-08-09_00-00-00").unwrap();
    r.write_resume(&[7u8; 8]).unwrap();

    assert!(r.spend_resume().unwrap(), "there was a resume to spend");

    assert_eq!(r.read_resume().unwrap(), None, "the resume outlived it");
    assert_eq!(
        r.list().unwrap().len(),
        1,
        "spending the resume took a saved state with it"
    );
}

#[test]
fn spending_a_resume_that_is_not_there_is_not_an_error() {
    let d = tempdir().unwrap();
    let r = StateRing::new(d.path(), Platform::Gba, Core::Mgba, "Never Played");
    assert!(!r.spend_resume().unwrap());
    r.write_resume(&[7u8; 8]).unwrap();
    assert!(r.spend_resume().unwrap());
    assert!(
        !r.spend_resume().unwrap(),
        "the same resume was spent twice"
    );
}
