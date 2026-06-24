use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use thiserror::Error;

/// Crate-wide result type.
pub type SkillmgrResult<T> = Result<T, SkillmgrError>;

/// Errors returned by skillmgr library operations.
#[derive(Debug, Error)]
pub enum SkillmgrError {
    /// A planned command exists in the CLI but has not been implemented yet.
    #[error("command `{command}` is not implemented yet")]
    NotImplemented {
        /// Command name.
        command: String,
    },

    /// The store path could not be resolved.
    #[error("could not resolve the skillmgr store path: {reason}")]
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

impl SkillmgrError {
    /// Exit code for this error.
    #[must_use]
    pub fn exit_code(&self) -> SkillmgrExitCode {
        match self {
            Self::NotImplemented { .. } => SkillmgrExitCode::ExpectedFailure,
            Self::StorePath { .. } | Self::InvalidStorePath { .. } | Self::CommandFailed { .. } => {
                SkillmgrExitCode::EnvironmentProblem
            }
            Self::TomlSerialize(_) | Self::Json(_) => SkillmgrExitCode::ExpectedFailure,
            Self::Io(_) => SkillmgrExitCode::EnvironmentProblem,
        }
    }
}

/// Public process exit code policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillmgrExitCode {
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

impl From<SkillmgrExitCode> for ExitCode {
    fn from(code: SkillmgrExitCode) -> Self {
        Self::from(code as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_should_use_expected_failure_exit_code() {
        let error = SkillmgrError::NotImplemented {
            command: "sync".to_owned(),
        };

        assert_eq!(error.exit_code(), SkillmgrExitCode::ExpectedFailure);
    }
}
