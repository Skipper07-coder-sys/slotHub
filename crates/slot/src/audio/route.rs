use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static WANT_BLUETOOTH: AtomicBool = AtomicBool::new(false);
static ON_BLUETOOTH: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(0);

pub fn want_bluetooth(on: bool) {
    if WANT_BLUETOOTH.swap(on, Ordering::SeqCst) != on {
        GENERATION.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn wants_bluetooth() -> bool {
    WANT_BLUETOOTH.load(Ordering::SeqCst)
}

pub fn on_bluetooth() -> bool {
    ON_BLUETOOTH.load(Ordering::SeqCst)
}

pub(crate) fn set_on_bluetooth(on: bool) {
    ON_BLUETOOTH.store(on, Ordering::SeqCst);
}

pub(crate) fn generation() -> u32 {
    GENERATION.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_of_mind_moves_the_output() {
        want_bluetooth(false);
        let at = generation();
        want_bluetooth(false);
        assert_eq!(generation(), at);
        want_bluetooth(true);
        assert_eq!(generation(), at + 1);
        assert!(wants_bluetooth());
        want_bluetooth(true);
        assert_eq!(generation(), at + 1);
        want_bluetooth(false);
        assert_eq!(generation(), at + 2);
    }
}
