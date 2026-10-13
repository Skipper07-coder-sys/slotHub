//! Builds the vendored rcheevos C library: the client, the server API, the
//! achievement runtime and MD5. Hashing and RAIntegration are left out.

use std::path::Path;

const SOURCES: &[&str] = &[
    "src/rc_client.c",
    "src/rc_compat.c",
    "src/rc_util.c",
    "src/rc_version.c",
    "src/rapi/rc_api_common.c",
    "src/rapi/rc_api_info.c",
    "src/rapi/rc_api_runtime.c",
    "src/rapi/rc_api_user.c",
    "src/rcheevos/alloc.c",
    "src/rcheevos/condition.c",
    "src/rcheevos/condset.c",
    "src/rcheevos/consoleinfo.c",
    "src/rcheevos/format.c",
    "src/rcheevos/lboard.c",
    "src/rcheevos/memref.c",
    "src/rcheevos/operand.c",
    "src/rcheevos/rc_validate.c",
    "src/rcheevos/richpresence.c",
    "src/rcheevos/runtime.c",
    "src/rcheevos/runtime_progress.c",
    "src/rcheevos/trigger.c",
    "src/rcheevos/value.c",
    "src/rhash/md5.c",
];

fn main() {
    let root = Path::new("vendor/rcheevos");
    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .include(root.join("src"))
        .warnings(false);
    for src in SOURCES {
        let path = root.join(src);
        println!("cargo:rerun-if-changed={}", path.display());
        build.file(path);
    }
    println!("cargo:rerun-if-changed=vendor/rcheevos/include");
    build.compile("rcheevos");
}
