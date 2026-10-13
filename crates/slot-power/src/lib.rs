mod device;
mod motor;
mod power;
mod sim;

pub use device::{
    clear_restart_mark, gpu_floor, has_bit, mark_restart, motor_change, pick_governor, read_only,
    read_only_in, record_first_frame, rumble_node, trace_first_frame, uptime_seconds,
    DevicePlatform,
};
pub use motor::Motor;
pub use power::Power;
pub use sim::SimPlatform;

use std::path::Path;
use std::time::Duration;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Charge {
    Unknown,
    Discharging,
    Charging,
    Full,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Battery {
    pub percent: u8,
    pub charge: Charge,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum LedState {
    Off,
    Running,
    Low,
    Charging,
    Charged,
}

/// How hard the CPU runs. slot asks for the least that keeps the game smooth.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum CpuMode {
    /// Clocks follow the load: menus and normal play.
    Balanced,
    /// Top clock held: fast-forward and link play, where any dip shows.
    Full,
    /// Lowest clock: the lid is shut.
    Rest,
}

impl CpuMode {
    /// The cpufreq governors for this mode, best first.
    pub fn governors(self) -> &'static [&'static str] {
        match self {
            CpuMode::Balanced => &["schedutil", "ondemand", "interactive"],
            CpuMode::Full => &["performance"],
            CpuMode::Rest => &["powersave"],
        }
    }
}

pub trait Platform: Send {
    fn set_backlight(&mut self, step: u8);
    fn battery(&self) -> Option<Battery>;
    fn charge(&self) -> Charge;
    fn set_led(&mut self, state: LedState);
    fn poweroff(&mut self) -> !;
    fn restart(&mut self) -> !;
    fn root(&self) -> &Path;
    fn now(&self) -> i64;
    fn set_clock(&mut self, secs: i64);
    fn set_rumble(&mut self, strength: u16);

    fn relink_adb(&mut self) -> bool {
        false
    }

    fn headphones(&self) -> bool {
        false
    }

    fn card_read_only(&self) -> bool {
        false
    }

    fn set_cpu_mode(&mut self, _mode: CpuMode) {}

    /// Puts the whole handheld to sleep in RAM until the lid, the power key or
    /// an alarm `wake_after_s` from now wakes it; returns once it is awake
    /// again, or false at once when it cannot sleep.
    fn suspend(&mut self, _wake_after_s: u32) -> bool {
        false
    }
}

pub trait LidPolicy {
    fn on_close(&mut self);
    fn on_open(&mut self);
    fn timeout(&self) -> Duration;
}
