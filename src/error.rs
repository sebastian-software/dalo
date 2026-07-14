//! Error types and process exit-code mapping.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use thiserror::Error;

/// Crate-wide result type.
pub type DaloResult<T> = Result<T, DaloError>;

/// Errors returned by dalo library operations.
#[derive(Debug, Error)]
pub enum DaloError {
    /// An explicit automation check found a state requiring review.
    #[error("check failed: {reason}")]
    CheckFailed {
        /// Human-readable summary of the state that needs attention.
        reason: String,
    },
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

    /// The store has not been initialized yet.
    #[error(
        "dalo store is not initialized at `{path}`; run `dalo --store {} init` first",
        shell_quote_path(.path.as_path())
    )]
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

    /// An unconfigured source checkout needs an explicit recovery decision.
    #[error("source checkout already exists at `{path}`; {reason}")]
    SourceCheckoutExists {
        /// Existing checkout path.
        path: PathBuf,
        /// Actionable recovery guidance.
        reason: String,
    },

    /// A source ID is not a valid path component.
    #[error("invalid source id `{id}`: {reason}")]
    InvalidSourceId {
        /// Rejected source ID.
        id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// A Git URL embeds credentials that would otherwise leak into state or logs.
    #[error("Git URL contains userinfo; use an SSH URL or a credential helper instead")]
    UnsafeRemoteUrl,

    /// A source ID does not exist.
    #[error("unknown source `{source_id}`{hint}")]
    UnknownSource {
        /// Source ID.
        source_id: String,
        /// Known IDs or a recovery command when available.
        hint: String,
    },

    /// A source has local changes that block the operation.
    #[error(
        "source `{source_id}` has local changes at `{path}`; inspect with `git -C {} status`, then resolve or commit them before syncing",
        shell_quote_path(.path.as_path())
    )]
    DirtySource {
        /// Source ID.
        source_id: String,
        /// Source checkout path.
        path: PathBuf,
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
    #[error("skill `{skill}` was not found{hint}")]
    SkillNotFound {
        /// Skill slot, ID, or path.
        skill: String,
        /// Known IDs or a recovery command when available.
        hint: String,
    },

    /// A requested local instruction pack could not be found.
    #[error("instruction pack `{pack_id}` was not found; create `{path}` before enabling it")]
    InstructionPackNotFound {
        /// Requested pack ID.
        pack_id: String,
        /// Expected local pack file.
        path: PathBuf,
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

    /// A persisted store file uses an unsupported schema version.
    #[error(
        "unsupported schema version {version} in `{path}`; this dalo supports version {supported}"
    )]
    UnsupportedSchema {
        /// Store file path.
        path: PathBuf,
        /// Persisted version.
        version: u32,
        /// Supported version.
        supported: u32,
    },

    /// Additive metadata from a newer binary cannot be merged losslessly.
    #[error(
        "cannot merge additive state field `{field}` for materialization directory `{path}` because previous target records contain conflicting values; use a newer dalo version or keep the targets on separate paths"
    )]
    StateMetadataConflict {
        /// Materialization directory whose records would be combined.
        path: PathBuf,
        /// Opaque additive field with conflicting values.
        field: String,
    },

    /// A system command failed.
    #[error("command `{program}` failed with status {status}: {stderr}")]
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

    /// The state file exists but could not be parsed and can be regenerated.
    #[error(
        "could not parse state file `{path}`: {reason}; run `dalo init` to back it up and regenerate an empty state file"
    )]
    CorruptState {
        /// State file path.
        path: PathBuf,
        /// Parser error message.
        reason: String,
    },

    /// A catalog command targeted a source that is not a catalog source.
    #[error("source `{source_id}` is not a catalog source")]
    NotACatalogSource {
        /// Source ID.
        source_id: String,
    },

    /// A catalog skill selector matches more than one candidate.
    #[error("catalog skill reference `{reference}` is ambiguous; matches: {matches}")]
    AmbiguousSkillReference {
        /// User-provided selector.
        reference: String,
        /// Human-readable matching candidates.
        matches: String,
    },

    /// Managed instruction block markers are malformed.
    #[error("malformed instruction block for `{pack_id}`: {reason}")]
    MalformedInstructionBlock {
        /// Instruction pack ID.
        pack_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// Terminal or filesystem I/O failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Render a path as one POSIX shell word for copyable recovery commands.
#[must_use]
pub(crate) fn shell_quote_path(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

impl DaloError {
    /// Build an unknown-source error with concise recovery guidance.
    #[must_use]
    pub fn unknown_source(source_id: impl Into<String>, known_sources: Vec<String>) -> Self {
        Self::UnknownSource {
            source_id: source_id.into(),
            hint: known_ids_hint("sources", known_sources, "dalo source list"),
        }
    }

    /// Build an unknown-skill error with concise recovery guidance.
    #[must_use]
    pub fn skill_not_found(
        skill: impl Into<String>,
        known_skills: Vec<String>,
        next_command: impl AsRef<str>,
    ) -> Self {
        Self::SkillNotFound {
            skill: skill.into(),
            hint: known_ids_hint("skills", known_skills, next_command.as_ref()),
        }
    }

    /// Exit code for this error.
    #[must_use]
    pub fn exit_code(&self) -> DaloExitCode {
        match self {
            Self::CheckFailed { .. } | Self::NotImplemented { .. } => DaloExitCode::ExpectedFailure,
            Self::StoreNotInitialized { .. }
            | Self::UnknownTarget { .. }
            | Self::TargetPathRequired { .. }
            | Self::SourceAlreadyExists { .. }
            | Self::SourceCheckoutExists { .. }
            | Self::InvalidSourceId { .. }
            | Self::UnsafeRemoteUrl
            | Self::NotACatalogSource { .. }
            | Self::AmbiguousSkillReference { .. }
            | Self::UnknownSource { .. }
            | Self::SkillNotFound { .. }
            | Self::InstructionPackNotFound { .. }
            | Self::AdoptionDestinationExists { .. }
            | Self::UnsupportedSchema { .. }
            | Self::FileParse { .. }
            | Self::CorruptState { .. }
            | Self::LocalSourcePriorityFixed { .. } => DaloExitCode::ExpectedFailure,
            Self::DirtySource { .. }
            | Self::StoreLocked { .. }
            | Self::StateMetadataConflict { .. }
            | Self::MalformedInstructionBlock { .. } => DaloExitCode::UnsafeState,
            Self::StorePath { .. } | Self::InvalidStorePath { .. } | Self::CommandFailed { .. } => {
                DaloExitCode::EnvironmentProblem
            }
            Self::TomlSerialize(_) | Self::Json(_) => DaloExitCode::ExpectedFailure,
            Self::Io(_) => DaloExitCode::EnvironmentProblem,
        }
    }
}

fn known_ids_hint(label: &str, mut ids: Vec<String>, next_command: &str) -> String {
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        format!("; run `{next_command}`")
    } else if ids.len() <= 6 {
        format!("; known {label}: {}", ids.join(", "))
    } else {
        format!("; run `{next_command}`")
    }
}

/// Public process exit code policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DaloExitCode {
    /// Success.
    Success = 0,
    /// Expected actionable failure.
    ExpectedFailure = 1,
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

impl DaloExitCode {
    /// Machine-readable exit-code label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ExpectedFailure => "expected_failure",
            Self::UnsafeState => "unsafe_state",
            Self::EnvironmentProblem => "environment_problem",
        }
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
            path: PathBuf::from("/tmp/store with $(shell)/it's"),
        }));

        assert_eq!(
            error.to_string(),
            "dalo store is not initialized at `/tmp/store with $(shell)/it's`; run `dalo --store '/tmp/store with $(shell)/it'\"'\"'s' init` first"
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
            hint: String::new(),
        }));

        assert_eq!(error.to_string(), "unknown source `company`");
    }

    #[test]
    fn dirty_source_should_render_source_id() {
        let error = err::<()>(Err(DaloError::DirtySource {
            source_id: "company".to_owned(),
            path: PathBuf::from("/tmp/store/sources/company/checkout"),
        }));

        assert_eq!(
            error.to_string(),
            "source `company` has local changes at `/tmp/store/sources/company/checkout`; inspect with `git -C '/tmp/store/sources/company/checkout' status`, then resolve or commit them before syncing"
        );
    }

    #[test]
    fn shell_quote_path_should_keep_metacharacters_in_one_literal_word() {
        assert_eq!(
            shell_quote_path(Path::new("/tmp/Jane's $(checkout); rm -rf nope")),
            "'/tmp/Jane'\"'\"'s $(checkout); rm -rf nope'"
        );
    }

    #[test]
    fn skill_not_found_should_render_selector() {
        let error = err::<()>(Err(DaloError::SkillNotFound {
            skill: "review".to_owned(),
            hint: String::new(),
        }));

        assert_eq!(error.to_string(), "skill `review` was not found");
    }

    #[test]
    fn instruction_pack_not_found_should_render_expected_path() {
        let error = err::<()>(Err(DaloError::InstructionPackNotFound {
            pack_id: "house-style".to_owned(),
            path: PathBuf::from("/tmp/store/local/instructions/house-style.md"),
        }));

        assert_eq!(
            error.to_string(),
            "instruction pack `house-style` was not found; create `/tmp/store/local/instructions/house-style.md` before enabling it"
        );
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
    fn unsupported_schema_should_render_versions_and_use_expected_failure() {
        let error = DaloError::UnsupportedSchema {
            path: PathBuf::from("/tmp/store/state.toml"),
            version: 999,
            supported: 1,
        };

        assert_eq!(error.exit_code(), DaloExitCode::ExpectedFailure);
        assert_eq!(
            error.to_string(),
            "unsupported schema version 999 in `/tmp/store/state.toml`; this dalo supports version 1"
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
            "command `git` failed with status exit status: 1: boom"
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
            DaloError::SourceCheckoutExists {
                path: PathBuf::from("/tmp/store/sources/company/checkout"),
                reason: "move or remove it before retrying".to_owned(),
            },
            DaloError::UnknownSource {
                source_id: "company".to_owned(),
                hint: String::new(),
            },
            DaloError::SkillNotFound {
                skill: "review".to_owned(),
                hint: String::new(),
            },
            DaloError::InstructionPackNotFound {
                pack_id: "house-style".to_owned(),
                path: PathBuf::from("/tmp/store/local/instructions/house-style.md"),
            },
            DaloError::AdoptionDestinationExists {
                path: PathBuf::from("/tmp/local/review"),
            },
            DaloError::UnsupportedSchema {
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
                path: PathBuf::from("/tmp/store/sources/company/checkout"),
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
