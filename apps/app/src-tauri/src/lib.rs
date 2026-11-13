// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Stepclimb app shell: one window loading the built web app (`web/dist`), or the Vite dev
//! server under `tauri dev`. Server discovery, PKCE sign-in, the keychain, offline downloads and
//! notifications are M5 (`docs/specs/m5.md`); until then the shell has no native features.

/// Start the app.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    if let Err(error) = tauri::Builder::default().run(tauri::generate_context!()) {
        eprintln!("stepclimb app: {error}");
        std::process::exit(1);
    }
}
