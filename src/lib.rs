//! Core library for the `dalo` command-line application.
//!
//! The Rust library API is not a semver contract; the CLI, its exit codes, its
//! `--json` output, and the files Dalo persists are. Modules, types, and
//! signatures here may change in any release, including a patch.
//!
//! The library owns the domain model and operations so CLI handlers stay thin
//! and command behavior can be tested without spawning the binary. To integrate
//! with Dalo, call the CLI and read its `--json` reports and exit codes: that is
//! the documented, supported surface.
//!
//! The full stability contract, including the three tiers, the change policy,
//! the persisted schema versions, and the supported platforms, lives in
//! [`docs/compatibility.md`](https://dalo.sh/docs/compatibility.html) and is
//! recorded in ADR 0008.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// dalo relies on `std::os::unix` symlink APIs and `$HOME` resolution. Fail the
// build early on unsupported platforms instead of with a deep type error.
#[cfg(not(unix))]
compile_error!(
    "dalo currently targets Unix-like platforms (Linux, macOS); Windows is not yet supported"
);

pub mod adopt;
pub mod agent;
pub mod approval;
pub mod audit;
pub mod autosync;
pub mod catalog;
// CLI plumbing: hidden from the rendered docs, not part of any contract.
#[doc(hidden)]
pub mod cli;
pub mod config;
pub mod delivery;
pub mod doctor;
pub mod error;
pub mod git;
pub mod hook;
pub mod hook_dispatch;
pub mod hook_sidecar;
pub mod hook_sync;
pub mod instructions;
pub mod inventory;
pub mod lockfile;
pub mod materialize;
pub mod package_validation;
pub mod plan;
pub mod plugin;
pub mod plugin_projection;
pub mod plugin_review;
pub mod resolver;
pub mod source;
pub mod status;
pub mod store;
pub mod target;
pub mod team_manifest;
// CLI plumbing: hidden from the rendered docs, not part of any contract.
#[doc(hidden)]
pub mod term;
pub mod tool;
// CLI plumbing: hidden from the rendered docs, not part of any contract.
#[doc(hidden)]
pub mod update;

pub use error::{DaloError, DaloResult};
