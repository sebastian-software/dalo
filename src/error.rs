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

    /// A source ID is not a valid path component.
    #[error("invalid source id `{id}`: {reason}")]
    InvalidSourceId {
        /// Rejected source ID.
        id: String,
        /// Human-readable reason.
        reason: String,
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

    /// The local source priority is fixed and cannot be changed.
    #[error(
        "source `{source_id}` is the local source; its priority is fixed and cannot be changed"
    )]
    LocalSourcePriorityFixed {
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

    /// A store file exists but could not be parsed.
    #[error("could not parse `{path}`: {reason}")]
    FileParse {
        /// File path.
        path: PathBuf,
        /// Parser error message.
        reason: String,
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
            | Self::InvalidSourceId { .. }
            | Self::UnknownSource { .. }
            | Self::SkillNotFound { .. }
            | Self::AdoptionDestinationExists { .. }
            | Self::UnsupportedLockSchema { .. }
            | Self::FileParse { .. }
            | Self::LocalSourcePriorityFixed { .. }
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

    fn err<T>(result: DaloResult<T>) -> DaloError
    where
        T: std::fmt::Debug,
    {
        result.expect_err("operation should fail")
    }

    #[test]
    fn not_implemented_should_use_expected_failure_exit_code() {
        let error = DaloError::NotImplemented {
            command: "sync".to_owned(),
        };

        assert_eq!(error.exit_code(), DaloExitCode::ExpectedFailure);
    }

    #[test]
    fn not_implemented_should_render_command_name() {
        let error = err::<()>(Err(DaloError::NotImplemented {
            command: "sync".to_owned(),
        }));

        assert_eq!(error.to_string(), "command `sync` is not implemented yet");
    }

    #[test]
    fn store_path_should_render_reason() {
        let error = err::<()>(Err(DaloError::StorePath {
            reason: "HOME is not set".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "could not resolve the dalo store path: HOME is not set"
        );
    }

    #[test]
    fn invalid_store_path_should_render_path_and_reason() {
        let error = err::<()>(Err(DaloError::InvalidStorePath {
            path: PathBuf::from("/tmp/store"),
            reason: "path is empty".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "invalid store path `/tmp/store`: path is empty"
        );
    }

    #[test]
    fn store_not_initialized_should_render_path() {
        let error = err::<()>(Err(DaloError::StoreNotInitialized {
            path: PathBuf::from("/tmp/store"),
        }));

        assert_eq!(
            error.to_string(),
            "dalo store is not initialized at `/tmp/store`; run `dalo init` first"
        );
    }

    #[test]
    fn unknown_target_should_render_target_id() {
        let error = err::<()>(Err(DaloError::UnknownTarget {
            target: "codex".to_owned(),
        }));

        assert_eq!(error.to_string(), "unknown target `codex`");
    }

    #[test]
    fn target_path_required_should_render_target_id() {
        let error = err::<()>(Err(DaloError::TargetPathRequired {
            target: "generic".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "target `generic` requires an explicit path"
        );
    }

    #[test]
    fn source_already_exists_should_render_source_id() {
        let error = err::<()>(Err(DaloError::SourceAlreadyExists {
            source_id: "company".to_owned(),
        }));

        assert_eq!(error.to_string(), "source `company` already exists");
    }

    #[test]
    fn invalid_source_id_should_render_id_and_reason() {
        let error = err::<()>(Err(DaloError::InvalidSourceId {
            id: "../../evil".to_owned(),
            reason: "contains `/`".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "invalid source id `../../evil`: contains `/`"
        );
    }

    #[test]
    fn invalid_source_id_should_use_expected_failure_exit_code() {
        let error = DaloError::InvalidSourceId {
            id: "a/b".to_owned(),
            reason: "contains `/`".to_owned(),
        };

        assert_eq!(error.exit_code(), DaloExitCode::ExpectedFailure);
    }

    #[test]
    fn unknown_source_should_render_source_id() {
        let error = err::<()>(Err(DaloError::UnknownSource {
            source_id: "company".to_owned(),
        }));

        assert_eq!(error.to_string(), "unknown source `company`");
    }

    #[test]
    fn dirty_source_should_render_source_id() {
        let error = err::<()>(Err(DaloError::DirtySource {
            source_id: "company".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "source `company` has local changes; resolve or commit them before syncing"
        );
    }

    #[test]
    fn skill_not_found_should_render_selector() {
        let error = err::<()>(Err(DaloError::SkillNotFound {
            skill: "review".to_owned(),
        }));

        assert_eq!(error.to_string(), "skill `review` was not found");
    }

    #[test]
    fn adoption_destination_exists_should_render_path() {
        let error = err::<()>(Err(DaloError::AdoptionDestinationExists {
            path: PathBuf::from("/tmp/local/review"),
        }));

        assert_eq!(
            error.to_string(),
            "local skill destination `/tmp/local/review` already exists; dalo will not overwrite it"
        );
    }

    #[test]
    fn store_locked_should_render_lock_path() {
        let error = err::<()>(Err(DaloError::StoreLocked {
            path: PathBuf::from("/tmp/store/.lock"),
        }));

        assert_eq!(
            error.to_string(),
            "another dalo operation is running (lock `/tmp/store/.lock`)"
        );
    }

    #[test]
    fn unsupported_lock_schema_should_render_versions() {
        let error = err::<()>(Err(DaloError::UnsupportedLockSchema {
            path: PathBuf::from("/tmp/store/lock.toml"),
            version: 999,
            supported: 1,
        }));

        assert_eq!(
            error.to_string(),
            "unsupported lock schema version 999 in `/tmp/store/lock.toml`; this dalo supports version 1"
        );
    }

    #[test]
    fn command_failed_should_render_program_and_status() {
        let error = err::<()>(Err(DaloError::CommandFailed {
            program: "git".to_owned(),
            args: "pull".to_owned(),
            cwd: PathBuf::from("/tmp/checkout"),
            status: "exit status: 1".to_owned(),
            stderr: "boom".to_owned(),
        }));

        assert_eq!(
            error.to_string(),
            "command `git pull` failed in `/tmp/checkout` with status exit status: 1: boom"
        );
    }

    #[test]
    fn expected_failure_exit_codes_should_cover_actionable_variants() {
        let cases = [
            DaloError::NotImplemented {
                command: "sync".to_owned(),
            },
            DaloError::StoreNotInitialized {
                path: PathBuf::from("/tmp/store"),
            },
            DaloError::UnknownTarget {
                target: "codex".to_owned(),
            },
            DaloError::TargetPathRequired {
                target: "generic".to_owned(),
            },
            DaloError::SourceAlreadyExists {
                source_id: "company".to_owned(),
            },
            DaloError::UnknownSource {
                source_id: "company".to_owned(),
            },
            DaloError::SkillNotFound {
                skill: "review".to_owned(),
            },
            DaloError::AdoptionDestinationExists {
                path: PathBuf::from("/tmp/local/review"),
            },
            DaloError::UnsupportedLockSchema {
                path: PathBuf::from("/tmp/store/lock.toml"),
                version: 999,
                supported: 1,
            },
        ];

        assert!(
            cases
                .iter()
                .all(|error| error.exit_code() == DaloExitCode::ExpectedFailure)
        );
    }

    #[test]
    fn unsafe_state_exit_codes_should_cover_blocking_variants() {
        let cases = [
            DaloError::DirtySource {
                source_id: "company".to_owned(),
            },
            DaloError::StoreLocked {
                path: PathBuf::from("/tmp/store/.lock"),
            },
        ];

        assert!(
            cases
                .iter()
                .all(|error| error.exit_code() == DaloExitCode::UnsafeState)
        );
    }

    #[test]
    fn environment_problem_exit_codes_should_cover_dependency_variants() {
        let cases = [
            DaloError::StorePath {
                reason: "HOME is not set".to_owned(),
            },
            DaloError::InvalidStorePath {
                path: PathBuf::from("/tmp/store"),
                reason: "path is empty".to_owned(),
            },
            DaloError::CommandFailed {
                program: "git".to_owned(),
                args: "pull".to_owned(),
                cwd: PathBuf::from("/tmp/checkout"),
                status: "exit status: 1".to_owned(),
                stderr: "boom".to_owned(),
            },
            DaloError::Io(io::Error::other("disk full")),
        ];

        assert!(
            cases
                .iter()
                .all(|error| error.exit_code() == DaloExitCode::EnvironmentProblem)
        );
    }

    #[test]
    fn exit_code_should_convert_to_process_exit_code() {
        let code: ExitCode = DaloExitCode::UnsafeState.into();

        assert_eq!(code, ExitCode::from(3));
    }
}
