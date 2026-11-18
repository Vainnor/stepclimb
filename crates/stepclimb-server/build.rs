// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recompiles the crate when `web/dist` appears or changes, so the build order of the server and
//! the web app does not matter.
//!
//! `spa::WebDist` resolves its folder when the derive runs. A debug build compiled while the
//! folder is missing keeps a path rust-embed could not canonicalize, and refuses every file at
//! request time. Cargo re-runs this script on every build while the path is missing, and after
//! that whenever a file under it changes; each run recompiles the crate.
//!
//! A recompile alone is not enough under a caching rustc wrapper such as sccache: the sources
//! and arguments are the same, so the wrapper can hand back the compile made without the folder.
//! `STEPCLIMB_WEB_DIST` (`present` or `missing`), which `spa.rs` reads with `env!`, puts the
//! folder's existence among the compiler's inputs, so the two compiles differ.

use std::path::Path;

const WEB_DIST: &str = "../../web/dist";

fn main() {
    println!("cargo::rerun-if-changed={WEB_DIST}");
    let state = if Path::new(WEB_DIST).is_dir() {
        "present"
    } else {
        "missing"
    };
    println!("cargo::rustc-env=STEPCLIMB_WEB_DIST={state}");
}
