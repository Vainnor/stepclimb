// SPDX-License-Identifier: AGPL-3.0-or-later
//! Optional connectors (VATSIM, VATUSA first) that use only the public API, as any outside
//! system would. It depends on no other workspace crate (`bin/check-layering.sh`), and only
//! `stepclimb-server` depends on it, behind a cargo feature per connector.
//!
//! Today it holds one piece of data: the VATSIM Connect sign-in preset ([`vatsim::PRESET`],
//! feature `vatsim`; docs/specs/m0/06-oauth2-and-vatsim-preset.md Q4). The connectors that sync
//! rosters and ratings are scheduled for M4 (`docs/specs/m4.md`).

#[cfg(feature = "vatsim")]
pub mod vatsim {
    //! VATSIM Connect.

    /// The VATSIM Connect sign-in preset as TOML: `kind`, `key` and `label`, and under
    /// `config` the members of an `oauth2` provider's config it supplies. The server reads it
    /// (`stepclimb_server::presets`); this crate only holds the data.
    pub const PRESET: &str = include_str!("vatsim/preset.toml");
}
