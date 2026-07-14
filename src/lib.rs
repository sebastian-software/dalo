//! Core library for the `dalo` command-line application.
//!
//! The library owns the domain model and operations so CLI handlers stay thin
//! and command behavior can be tested without spawning the binary.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

// dalo relies on `std::os::unix` symlink APIs and `$HOME` resolution. Fail the
// build early on unsupported platforms instead of with a deep type error.
#[cfg(not(unix))]
compile_error!(
    "dalo currently targets Unix-like platforms (Linux, macOS); Windows is not yet supported"
);

pub mod adopt;
pub mod approval;
pub mod audit;
pub mod catalog;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod error;
pub mod git;
pub mod instructions;
pub mod inventory;
pub mod lockfile;
pub mod materialize;
pub mod resolver;
pub mod source;
pub mod status;
pub mod store;
pub mod target;
pub mod term;

pub use error::{DaloError, DaloResult};
