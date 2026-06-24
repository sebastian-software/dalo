//! Core library for the `skillmgr` command-line application.
//!
//! The library owns the domain model and operations so CLI handlers stay thin
//! and command behavior can be tested without spawning the binary.

#![forbid(unsafe_code)]

pub mod adopt;
pub mod cli;
pub mod config;
pub mod doctor;
pub mod error;
pub mod git;
pub mod inventory;
pub mod lockfile;
pub mod materialize;
pub mod resolver;
pub mod source;
pub mod status;
pub mod store;
pub mod target;

pub use error::{SkillmgrError, SkillmgrResult};
