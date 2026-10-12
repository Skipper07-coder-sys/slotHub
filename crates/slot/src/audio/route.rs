use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;

static WANT_BLUETOOTH: Mutex<Option<String>> = Mutex::new(None);
static ON_BLUETOOTH: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(0);

pub fn want_bluetooth(mac: Option<&str>) {
    let mut want = WANT_BLUETOOTH.lock().unwrap_or_else(|e| e.into_inner());
    if want.as_deref() != mac {
        *want = mac.map(str::to_owned);
        GENERATION.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn bluetooth_device() -> Option<String> {
    WANT_BLUETOOTH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn wants_bluetooth() -> bool {
    bluetooth_device().is_some()
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

// The H700's BlueALSA is older than 3.0 and will not pick a device for us, so
// every name carries the headphones' address. plug converts slot's 32768 Hz to
// a rate A2DP takes; the spellings differ only in how ALSA is asked to parse
// the slave's arguments.
pub fn pcm_names(mac: &str) -> Vec<String> {
    let slave = format!("bluealsa:DEV={mac},PROFILE=a2dp");
    vec![
        format!("plug:\"{slave}\""),
        format!("plug:{{SLAVE=\"{slave}\"}}"),
        slave,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_change_of_headphones_moves_the_output() {
        want_bluetooth(None);
        let at = generation();
        want_bluetooth(None);
        assert_eq!(generation(), at);
        want_bluetooth(Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(generation(), at + 1);
        assert_eq!(bluetooth_device().as_deref(), Some("AA:BB:CC:DD:EE:FF"));
        want_bluetooth(Some("AA:BB:CC:DD:EE:FF"));
        assert_eq!(generation(), at + 1);
        want_bluetooth(Some("11:22:33:44:55:66"));
        assert_eq!(generation(), at + 2);
        want_bluetooth(None);
        assert_eq!(generation(), at + 3);
        assert!(!wants_bluetooth());
    }

    #[test]
    fn every_pcm_name_names_the_headphones_and_a2dp() {
        let names = pcm_names("AA:BB:CC:DD:EE:FF");
        assert_eq!(
            names[0],
            "plug:\"bluealsa:DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp\""
        );
        for n in names {
            assert!(n.contains("DEV=AA:BB:CC:DD:EE:FF,PROFILE=a2dp"), "{n}");
        }
    }
}
