// SPDX-License-Identifier: AGPL-3.0-or-later
//! What a `Require<R, T>` extractor of the server checks: a permission of [`crate::perm`] or a
//! feature of [`crate::feature`] (docs/specs/m0/10-entitlements-framework.md B9).
//!
//! Both catalogs implement [`Requirement`], so handlers name `Require<perm::Y>` and
//! `Require<feature::X>` with the same extractor. The associated [`Requirement::Kind`] tells the
//! extractor which check to run. Rust cannot implement one trait for every `Permission` and for
//! every `Feature` at once, since nothing tells the compiler no type is both; dispatching on a
//! marker type per kind gives each check its own implementation.
//!
//! The catalog macros implement this trait for every entry. A test crate that defines a feature
//! of its own (through `feature::TestSeal`) implements it with `Kind = FeatureKind`. The
//! extractor's checks also require the sealed `Permission` or `Feature` trait, so naming a kind
//! on any other type gives it no check to run.

/// A permission or a feature a `Require` extractor checks.
pub trait Requirement {
    /// [`PermissionKind`] or [`FeatureKind`].
    type Kind;
}

/// The kind of every permission in [`crate::perm::ALL`]. A type-level marker; it has no value.
#[derive(Debug)]
pub enum PermissionKind {}

/// The kind of every feature in [`crate::feature::ALL`], and of a test crate's own feature. A
/// type-level marker; it has no value.
#[derive(Debug)]
pub enum FeatureKind {}
