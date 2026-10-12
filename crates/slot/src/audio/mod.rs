mod alsa;
#[cfg(feature = "host")]
mod host;
mod ring;
pub mod route;
mod sfx;
mod silence;
mod sink;
mod stub;
pub mod volume;

pub use alsa::AlsaSink;
#[cfg(feature = "host")]
pub use host::HostAudio;
pub use ring::{ring_capacity, Ring};
pub use sfx::Sfx;
pub use silence::Silence;
pub use sink::{AudioError, AudioSink};
pub use stub::StubSink;

pub const GBA_HZ: u32 = 32_768;

use std::sync::{Arc, Mutex};

/// A sink that opens its device in the background: its ring is there at
/// once, and stays silent (sample rate 0) until the device is up. Opening the
/// sound card early in boot takes a few hundred ms that the shelf need not
/// wait for.
pub fn opening_sink() -> Box<dyn AudioSink> {
    opening(open_sink())
}

fn opening(mut sink: Box<dyn AudioSink>) -> Box<dyn AudioSink> {
    let ring = sink.ring();
    let held: Arc<Mutex<Option<Box<dyn AudioSink>>>> = Arc::default();
    let into = held.clone();
    let spawned = std::thread::Builder::new()
        .name("slot-audio-open".into())
        .spawn(move || {
            if let Err(e) = sink.open(GBA_HZ) {
                eprintln!("slot: audio: {e}");
            }
            eprintln!("slot: audio open at {:.1} ms", crate::boot_time::ms());
            *into.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink);
        });
    if let Err(e) = spawned {
        eprintln!("slot: audio: open thread failed to start: {e}");
        return opened_sink();
    }
    Box::new(Opening { ring, held })
}

struct Opening {
    ring: Arc<Ring>,
    /// The sink once its device is open; it lives as long as this does.
    held: Arc<Mutex<Option<Box<dyn AudioSink>>>>,
}

impl AudioSink for Opening {
    fn open(&mut self, sample_rate: u32) -> Result<(), AudioError> {
        match self.held.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            Some(sink) => sink.open(sample_rate),
            None => Ok(()),
        }
    }

    fn ring(&self) -> Arc<Ring> {
        self.ring.clone()
    }
}

pub fn opened_sink() -> Box<dyn AudioSink> {
    let mut sink = open_sink();
    if let Err(e) = sink.open(GBA_HZ) {
        eprintln!("slot: audio: {e}");
    }
    sink
}

#[cfg(feature = "host")]
pub fn open_sink() -> Box<dyn AudioSink> {
    if std::env::var_os("SLOT_SILENT").is_some() {
        return Box::new(StubSink::draining());
    }
    Box::new(HostAudio::new())
}

#[cfg(not(feature = "host"))]
pub fn open_sink() -> Box<dyn AudioSink> {
    Box::new(AlsaSink::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_opening_sink_shares_its_ring_and_comes_up_in_the_background() {
        let stub = StubSink::draining();
        let ring = stub.ring();
        let sink = opening(Box::new(stub));
        assert!(Arc::ptr_eq(&sink.ring(), &ring));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while ring.sample_rate() == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(ring.sample_rate(), GBA_HZ);
    }
}
