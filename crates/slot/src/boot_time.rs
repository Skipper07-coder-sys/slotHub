use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

pub fn mark(phase: &str) {
    if START.get().is_none() {
        // How long the system took to get here, before slot's own clock starts.
        let uptime = std::fs::read_to_string("/proc/uptime").unwrap_or_default();
        let uptime = uptime.split_whitespace().next().unwrap_or("?");
        eprintln!("slot: boot: clock starts at uptime {uptime} s");
    }
    eprintln!("slot: boot: {phase} at {:.1} ms", ms());
}

pub fn ms() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1e3
}
