// SPDX-License-Identifier: AGPL-3.0-or-later
// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    stepclimb_app_lib::run();
}
