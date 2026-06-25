use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use thiserror::Error;

/// Crate-wide result type.
pub type DaloResult<T> = Result<T, DaloError>;

/// Errors returned by dalo library operations.
#[derive(Debug, Error)]
pub enum DaloError {
    /// A planned command exists in the CLI but has not been implemented yet.
    #[error("command `{command}` is not implemented yet")]
    NotImplemented {
        /// Command name.
        command: String,
    },

    /// The store path could not be resolved.
    #[error("could not resolve the dalo store path: {reason}")]
    StorePath {
        /// Human-readable reason.
        reason: String,
    },

    /// The configured store path is invalid.
    #[error("invalid store path `{path}`: {reason}")]
    InvalidStorePath {
        /// Invalid path.
        path: PathBuf,
        /// Human-readable reason.
        reason: String,
    },

    /// TOML serialization failed.
    #[error(transparent)]
    TomlSerialize(#[from] toml::ser::Error),

    /// JSON serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// TOML deserialization failed.
    #[error(transparent)]
    TomlDeserialize(#[from] toml::de::Error),

    /// The store has not been initialized yet.
    #[error("dalo store is not initialized at `{path}`; run `dalo init` first")]
    StoreNotInitialized {
        /// Store path.
        path: PathBuf,
    },

    /// Target ID is unknown.
    #[error("unknown target `{target}`")]
    UnknownTarget {
        /// Target ID.
        target: String,
    },

    /// Target requires an explicit path.
    #[error("target `{target}` requires an explicit path")]
    TargetPathRequired {
        /// Target ID.
        target: String,
    },

    /// A source ID already exists.
    #[error("source `{source_id}` already exists")]
    SourceAlreadyExists {
        /// Source ID.
        source_id: String,
    },

    /// A source ID does not exist.
    #[error("unknown source `{source_id}`")]
    UnknownSource {
        /// Source ID.
        source_id: String,
    },

    /// A source has local changes that block the operation.
    #[error("source `{source_id}` has local changes; resolve or commit them before syncing")]
    DirtySource {
        /// Source ID.
        source_id: String,
    },

    /// A requested skill could not be found.
    #[error("skill `{skill}` was not found")]
    SkillNotFound {
        /// Skill slot, ID, or path.
        skill: String,
    },

    /// A local skill destination already exists.
    #[error("local skill destination `{path}` already exists; dalo will not overwrite it")]
    AdoptionDestinationExists {
        /// Existing destination path.
        path: PathBuf,
    },

    /// Another dalo operation is already running.
    #[error("another dalo operation is running (lock `{path}`)")]
    StoreLocked {
        /// Lock file path.
        path: PathBuf,
    },

    /// The persisted user lock uses an unsupported schema version.
    #[error(
        "unsupported lock schema version {version} in `{path}`; this dalo supports version {supported}"
    )]
    UnsupportedLockSchema {
        /// Lock file path.
        path: PathBuf,
        /// Persisted version.
        version: u32,
        /// Supported version.
        supported: u32,
    },

    /// A system command failed.
    #[error("command `{program} {args}` failed in `{cwd}` with status {status}: {stderr}")]
    CommandFailed {
        /// Program name.
        program: String,
        /// Shell-escaped-ish argument display for humans.
        args: String,
        /// Working directory.
        cwd: PathBuf,
        /// Exit status.
        status: String,
        /// Standard error output.
        stderr: String,
    },

    /// Terminal or filesystem I/O failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

impl DaloError {
    /// Exit code for this error.
    #[must_use]
    pub fn exit_code(&self) -> DaloExitCode {
        match self {
            Self::NotImplemented { .. } => DaloExitCode::ExpectedFailure,
            Self::StoreNotInitialized { .. }
            | Self::UnknownTarget { .. }
            | Self::TargetPathRequired { .. }
            | Self::SourceAlreadyExists { .. }
            | Self::UnknownSource { .. }
            | Self::SkillNotFound { .. }
            | Self::AdoptionDestinationExists { .. }
            | Self::UnsupportedLockSchema { .. }
            | Self::TomlDeserialize(_) => DaloExitCode::ExpectedFailure,
            Self::DirtySource { .. } | Self::StoreLocked { .. } => DaloExitCode::UnsafeState,
            Self::StorePath { .. } | Self::InvalidStorePath { .. } | Self::CommandFailed { .. } => {
                DaloExitCode::EnvironmentProblem
            }
            Self::TomlSerialize(_) | Self::Json(_) => DaloExitCode::ExpectedFailure,
            Self::Io(_) => DaloExitCode::EnvironmentProblem,
        }
    }
}

/// Public process exit code policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaloExitCode {
    /// Success.
    Success = 0,
    /// Expected actionable failure.
    ExpectedFailure = 1,
    /// Invalid CLI usage.
    Usage = 2,
    /// Unsafe state blocked the operation.
    UnsafeState = 3,
    /// Dependency or environment problem.
    EnvironmentProblem = 4,
}

impl From<DaloExitCode> for ExitCode {
    fn from(code: DaloExitCode) -> Self {
        Self::from(code as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_should_use_expected_failure_exit_code() {
        let error = DaloError::NotImplemented {
            command: "sync".to_owned(),
        };

        assert_eq!(error.exit_code(), DaloExitCode::ExpectedFailure);
    }
}
