use slot_gfx::{Draw, TexId};
use slot_ui::{
    index_alpha, index_hold, marquee_shift, rail_glyph_face, shelf_letters, title_strip,
    title_window, LetterRail, INDEX_HOLD_MS, INDEX_IN_MS, INDEX_OUT_MS, MARQUEE_WAIT_MS,
    TITLE_WIN_W,
};

#[test]
fn the_index_comes_up_quickly_holds_then_goes() {
    let hold = INDEX_HOLD_MS;
    assert_eq!(index_alpha(0, 0, hold), 0.0);
    assert_eq!(index_alpha(INDEX_IN_MS, INDEX_IN_MS, hold), 1.0);
    assert_eq!(index_alpha(hold + 5000, hold - 1, hold), 1.0);
    let half = index_alpha(60_000, hold + INDEX_OUT_MS / 2, hold);
    assert!(half > 0.0 && half < 1.0, "the fade out jumped: {half}");
    assert_eq!(index_alpha(60_000, hold + INDEX_OUT_MS, hold), 0.0);
}

#[test]
fn a_scrolling_name_holds_the_index_until_it_has_gone_round_once() {
    let short = title_strip("Zelda");
    assert_eq!(index_hold(short.w), INDEX_HOLD_MS);
    let long = title_strip("Pokemon Mystery Dungeon Red Rescue Team");
    let hold = index_hold(long.w);
    assert!(hold > INDEX_HOLD_MS);
    // By the end of the hold the name is back where it started.
    let mut t = MARQUEE_WAIT_MS + 1;
    while marquee_shift(long.w, t) != 0 {
        t += 10;
    }
    assert!(
        t <= hold,
        "the name was still scrolling when it faded: {t} > {hold}"
    );
}

#[test]
fn a_shelf_has_each_first_letter_once_in_shelf_order() {
    let mut out = Vec::new();
    shelf_letters(
        ["1943", "Apotris", "apple", "Metroid", "Mega Man", "Zelda"],
        &mut out,
    );
    assert_eq!(out, vec!['#', 'A', 'M', 'Z']);
}

#[test]
fn the_rail_letters_are_drawn_from_the_thin_font() {
    let face = rail_glyph_face('M');
    assert!(face.w > 4 && face.h > 4);
    assert!(
        face.rgba.chunks(4).any(|p| p[3] > 200),
        "the M came out blank"
    );
}

fn rail_draws(letters: &[char], current: char) -> Vec<(f32, f32)> {
    let mut rail = LetterRail::default();
    rail.update(letters, Some(current), 1.0 / 60.0);
    let mut out = Vec::new();
    rail.draw(
        1.0,
        |c| Some((TexId::from_raw(c as usize), 30, 30)),
        &mut out,
    );
    out.iter()
        .filter_map(|d| match *d {
            Draw::Tex { x, alpha, .. } => Some((x, alpha)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_rail_shows_five_letters_brightest_in_the_middle() {
    let letters: Vec<char> = "#ABCDEFGHIJKLMNOPQRSTUVWXYZ".chars().collect();
    let drawn = rail_draws(&letters, 'M');
    assert_eq!(drawn.len(), 5);
    let middle = drawn.iter().fold(f32::MIN, |m, (_, a)| m.max(*a));
    let mut by_x = drawn.clone();
    by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(by_x[2].1, middle, "the current letter is not the brightest");
    assert!(
        by_x[0].1 < by_x[1].1 && by_x[4].1 < by_x[3].1,
        "the sides do not fade"
    );
}

#[test]
fn a_short_shelf_never_shows_a_letter_twice() {
    assert_eq!(rail_draws(&['A', 'B'], 'A').len(), 1);
    assert_eq!(rail_draws(&['A', 'B', 'C'], 'B').len(), 3);
}

#[test]
fn a_short_name_sits_still_in_the_slot() {
    let strip = title_strip("Zelda");
    assert_eq!(strip.w, TITLE_WIN_W);
    assert_eq!(marquee_shift(strip.w, 60_000), 0);
}

#[test]
fn a_long_name_waits_then_scrolls_round() {
    let strip = title_strip("Pokemon Mystery Dungeon Red Rescue Team");
    assert!(strip.w > TITLE_WIN_W);
    assert_eq!(marquee_shift(strip.w, MARQUEE_WAIT_MS - 1), 0);
    assert!(marquee_shift(strip.w, MARQUEE_WAIT_MS + 2000) > 0);
    let window = title_window(&strip, marquee_shift(strip.w, MARQUEE_WAIT_MS + 2000));
    assert_eq!(window.w, TITLE_WIN_W);
    assert_eq!(window.rgba.len(), (TITLE_WIN_W * window.h * 4) as usize);
}
