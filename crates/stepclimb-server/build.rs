// SPDX-License-Identifier: AGPL-3.0-or-later
//! Recompiles the crate when `web/dist` appears or changes, so the build order of the server and
//! the web app does not matter.
//!
//! `spa::WebDist` resolves its folder when the derive runs. A debug build compiled while the
//! folder is missing keeps a path rust-embed could not canonicalize, and refuses every file at
//! request time. Cargo re-runs this script on every build while the path is missing, and after
//! that whenever a file under it changes; each run recompiles the crate.

fn main() {
    println!("cargo::rerun-if-changed=../../web/dist");
}
