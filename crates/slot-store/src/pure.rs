use std::path::Path;

use crate::ini;

pub const PURE_FILE: &str = "Config/pure.txt";

const PURE_KEY: &str = "pure";

const BOM: char = '\u{feff}';

pub fn read_pure(root: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(root.join(PURE_FILE)) else {
        return false;
    };
    ini::parse(text.trim_start_matches(BOM))
        .get(PURE_KEY)
        .is_some_and(|v| is_on(v))
}

fn is_on(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "on" | "1" | "true" | "yes"
    )
}
