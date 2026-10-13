use slot_ui::{
    cheat_last_row_mid, cheat_pages, cheat_sheet_face, cheat_width, CHEAT_DESC_X, CHEAT_TEXT_PX,
    OUT_W, QUICK_EDGE, QUICK_SPLIT,
};

#[test]
fn pure_mode_drops_the_shortcuts_it_turns_off() {
    let all = cheat_pages(false);
    let pure = cheat_pages(true);
    assert!(all.iter().any(|p| p.title == "SAVE STATES"));
    assert!(
        pure.iter().all(|p| p.title != "SAVE STATES"),
        "pure mode has no save states, so no page for them"
    );
    let rows = |pages: &[slot_ui::CheatPage]| -> Vec<&str> {
        pages
            .iter()
            .flat_map(|p| p.rows.iter().map(|(_, what)| *what))
            .collect()
    };
    for gone in ["Save state", "Rewind", "Fast-forward"] {
        assert!(rows(&all).contains(&gone), "{gone} is missing outright");
        assert!(!rows(&pure).contains(&gone), "pure mode still lists {gone}");
    }
    assert!(pure.iter().all(|p| !p.rows.is_empty()));
}

#[test]
fn every_row_fits_its_column() {
    for pure in [false, true] {
        for page in cheat_pages(pure) {
            for (key, what) in &page.rows {
                assert!(
                    QUICK_EDGE + cheat_width(key, CHEAT_TEXT_PX) < CHEAT_DESC_X - 12.0,
                    "{key} runs into its description"
                );
                assert!(
                    CHEAT_DESC_X + cheat_width(what, CHEAT_TEXT_PX) <= OUT_W as f32 - QUICK_EDGE,
                    "{what} runs off the screen"
                );
            }
        }
    }
}

#[test]
fn the_longest_page_stays_clear_of_the_legend() {
    let bottom = cheat_last_row_mid(false) + CHEAT_TEXT_PX / 2.0;
    assert!(bottom < QUICK_SPLIT, "{bottom} reaches the legend");
}

#[test]
fn every_page_rasters_words() {
    for pure in [false, true] {
        for page in 0..cheat_pages(pure).len() {
            let f = cheat_sheet_face(page, pure);
            assert!(f.rgba.chunks(4).any(|p| p[3] > 0), "page {page} is blank");
        }
    }
}
