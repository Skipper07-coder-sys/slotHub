mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{clocked, tmp_root_with_carts};
use slot::app::App;
use slot_power::{Power, SimPlatform};

#[test]
fn adb_is_bound_again_every_few_seconds() {
    let d = tmp_root_with_carts(&["Emerald"]);
    clocked(d.path());
    let platform = SimPlatform::at(d.path().to_path_buf());
    let relinks = platform.relinks();
    let mut a = App::boot(d.path());
    a.set_power(Power::new(Box::new(platform), Duration::from_secs(300)));

    let start = a.now();
    a.tick_ms(start);
    let first = relinks.load(Ordering::Relaxed);
    assert!(first >= 1, "slot never checked the adb gadget");

    a.tick_ms(start + 1_000);
    assert_eq!(
        relinks.load(Ordering::Relaxed),
        first,
        "checked again within the poll interval"
    );
    a.tick_ms(start + 6_000);
    assert_eq!(relinks.load(Ordering::Relaxed), first + 1);
}
