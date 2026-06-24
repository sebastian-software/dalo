use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::error::{SkillmgrError, SkillmgrResult};

/// Parsed command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "skillmgr")]
#[command(version, about = "Git-backed skill management for AI agents.")]
pub struct Cli {
    /// Override the skillmgr store path.
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<PathBuf>,

    /// Emit machine-readable JSON where supported.
    #[arg(long, global = true)]
    pub json: bool,

    /// Accept safe interactive prompts.
    #[arg(long, global = true)]
    pub yes: bool,

    /// Show planned changes without mutating state.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Disable colored output.
    #[arg(long, global = true)]
    pub no_color: bool,

    /// Increase diagnostic verbosity.
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Command to execute.
    #[command(subcommand)]
    pub command: Option<Command>,
}

impl Cli {
    /// Parse command-line arguments from the current process.
    #[must_use]
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

/// Top-level command groups.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Initialize the skillmgr store.
    Init,
    /// Detect, link, or unlink agent targets.
    Target(TargetCommand),
    /// Manage skill sources.
    Source(SourceCommand),
    /// Show managed, unmanaged, and conflicted skill state.
    Status,
    /// Refresh clean tracking sources and materialize the resolved skill set.
    Sync,
    /// Adopt an unmanaged skill into the local source.
    Adopt(AdoptCommand),
    /// Run explicit safe repair helpers.
    Resolve(ResolveCommand),
    /// Diagnose store, target, Git, and lockfile health.
    Doctor,
}

/// `target` command group.
#[derive(Debug, Args)]
pub struct TargetCommand {
    /// Target subcommand.
    #[command(subcommand)]
    pub command: TargetSubcommand,
}

/// `target` subcommands.
#[derive(Debug, Subcommand)]
pub enum TargetSubcommand {
    /// Detect known agent targets.
    Detect,
    /// Link a target by ID.
    Link(TargetIdArg),
    /// Unlink a target by ID.
    Unlink(TargetIdArg),
}

/// Target ID argument.
#[derive(Debug, Args)]
pub struct TargetIdArg {
    /// Target ID, such as `codex`, `claude`, `openclaw`, `hermes`, or `generic`.
    pub target: String,
}

/// `source` command group.
#[derive(Debug, Args)]
pub struct SourceCommand {
    /// Source subcommand.
    #[command(subcommand)]
    pub command: SourceSubcommand,
}

/// `source` subcommands.
#[derive(Debug, Subcommand)]
pub enum SourceSubcommand {
    /// Add a local or team source.
    Add(SourceAddArgs),
    /// List configured sources.
    List,
    /// Change a source priority.
    Priority(SourcePriorityArgs),
}

/// Arguments for `source add`.
#[derive(Debug, Args)]
pub struct SourceAddArgs {
    /// Source ID.
    pub id: String,

    /// Git URL or local source path.
    pub location: String,
}

/// Arguments for `source priority`.
#[derive(Debug, Args)]
pub struct SourcePriorityArgs {
    /// Source ID.
    pub id: String,

    /// New priority. Lower numbers win.
    pub priority: i32,
}

/// Arguments for `adopt`.
#[derive(Debug, Args)]
pub struct AdoptCommand {
    /// Skill slot name or path to adopt.
    pub skill: String,
}

/// `resolve` command group.
#[derive(Debug, Args)]
pub struct ResolveCommand {
    /// Resolve subcommand.
    #[command(subcommand)]
    pub command: ResolveSubcommand,
}

/// Minimal V1 `resolve` subcommands.
#[derive(Debug, Subcommand)]
pub enum ResolveSubcommand {
    /// List known blockers and repairable states.
    List,
    /// Adopt the referenced unmanaged skill.
    Adopt(ResolveIdArg),
    /// Keep and protect the referenced unmanaged entry.
    Keep(ResolveIdArg),
    /// Remove an owned symlink by recorded ID.
    RemoveOwned(ResolveIdArg),
}

/// Resolver item ID argument.
#[derive(Debug, Args)]
pub struct ResolveIdArg {
    /// Diagnostic or state item ID.
    pub id: String,
}

/// Execute a parsed CLI command.
pub fn run_cli(cli: Cli) -> SkillmgrResult<()> {
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    Err(SkillmgrError::NotImplemented {
        command: command.name().to_owned(),
    })
}

impl Command {
    fn name(&self) -> &'static str {
        match self {
            Self::Init => "init",
            Self::Target(_) => "target",
            Self::Source(_) => "source",
            Self::Status => "status",
            Self::Sync => "sync",
            Self::Adopt(_) => "adopt",
            Self::Resolve(_) => "resolve",
            Self::Doctor => "doctor",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_cli_should_succeed_when_no_command_is_provided() {
        let cli = Cli {
            store: None,
            json: false,
            yes: false,
            dry_run: false,
            no_color: false,
            verbose: 0,
            command: None,
        };

        assert!(run_cli(cli).is_ok());
    }

    #[test]
    fn run_cli_should_return_not_implemented_for_stubbed_command() {
        let cli = Cli {
            store: None,
            json: false,
            yes: false,
            dry_run: false,
            no_color: false,
            verbose: 0,
            command: Some(Command::Status),
        };

        let error = run_cli(cli).expect_err("status should be stubbed");

        assert_eq!(error.to_string(), "command `status` is not implemented yet");
    }
}
