use std::io;
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
