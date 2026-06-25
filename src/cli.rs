use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::adopt;
use crate::doctor;
use crate::error::DaloResult;
use crate::lockfile;
use crate::materialize;
use crate::source;
use crate::status;
use crate::store;
use crate::target;

/// Parsed command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "dalo")]
#[command(version, about = "Git-backed skill management for AI agents.")]
pub struct Cli {
    /// Override the dalo store path.
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

    /// Command to execute.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Global command options after path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalOptions {
    /// Resolved store path.
    pub store: PathBuf,
    /// Emit JSON.
    pub json: bool,
    /// Accept safe prompts.
    pub yes: bool,
    /// Plan without mutating.
    pub dry_run: bool,
}

impl GlobalOptions {
    fn resolve(
        store: Option<&std::path::Path>,
        json: bool,
        yes: bool,
        dry_run: bool,
    ) -> DaloResult<Self> {
        Ok(Self {
            store: store::resolve_store_path(store)?,
            json,
            yes,
            dry_run,
        })
    }
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
    /// Initialize the dalo store.
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
    Link(TargetLinkArgs),
    /// Unlink a target by ID.
    Unlink(TargetIdArg),
}

/// Target link arguments.
#[derive(Debug, Args)]
pub struct TargetLinkArgs {
    /// Target ID, such as `codex`, `claude`, `openclaw`, `hermes`, or `generic`.
    pub target: String,

    /// Optional target path. Required for `generic`.
    pub path: Option<PathBuf>,
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
    /// Add a team source from a Git URL.
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

    /// Git URL of the team source.
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
pub fn run_cli(cli: Cli) -> DaloResult<()> {
    let Cli {
        store,
        json,
        yes,
        dry_run,
        command,
    } = cli;

    let Some(command) = command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    let options = GlobalOptions::resolve(store.as_deref(), json, yes, dry_run)?;

    match command {
        Command::Init => run_init(&options),
        Command::Target(command) => run_target(&options, command),
        Command::Source(command) => run_source(&options, command),
        Command::Status => run_status(&options),
        Command::Sync => run_sync(&options),
        Command::Adopt(command) => run_adopt(&options, command),
        Command::Resolve(command) => run_resolve(&options, command),
        Command::Doctor => run_doctor(&options),
    }
}

fn run_init(options: &GlobalOptions) -> DaloResult<()> {
    let report = store::init_store(options.store.clone(), options.dry_run)?;

    if options.json {
        print_json(&report)?;
    } else {
        status::print_init_report(&report);
    }

    Ok(())
}

fn run_status(options: &GlobalOptions) -> DaloResult<()> {
    let report = status::build_status_report(&options.store)?;

    if options.json {
        print_json(&report)?;
    } else {
        status::print_status_report(&report);
    }

    Ok(())
}

fn run_sync(options: &GlobalOptions) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    if !options.dry_run {
        source::refresh_tracking_team_sources(&paths)?;
    }
    let status_report = status::build_status_report(&options.store)?;
    let report = materialize::materialize(&paths, &status_report.resolution, options.dry_run)?;
    if !options.dry_run {
        let config = store::read_config(&paths)?;
        let lock =
            lockfile::build_user_lock(&config.sources, &status_report.resolution, Some(&report));
        store::write_user_lock(&paths, &lock)?;
    }

    if options.json {
        print_json(&report)?;
    } else {
        status::print_sync_report(&report);
    }

    Ok(())
}

fn run_source(options: &GlobalOptions, command: SourceCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    match command.command {
        SourceSubcommand::Add(args) => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report =
                source::add_team_source(&paths, &args.id, &args.location, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_source_add_report(&report);
            }
            Ok(())
        }
        SourceSubcommand::List => {
            let report = source::list_sources(&paths)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_source_list_report(&report);
            }
            Ok(())
        }
        SourceSubcommand::Priority(args) => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report =
                source::set_source_priority(&paths, &args.id, args.priority, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_source_priority_report(&report);
            }
            Ok(())
        }
    }
}

fn run_adopt(options: &GlobalOptions, command: AdoptCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    let report = adopt::adopt_skill(&paths, &command.skill, options.yes, options.dry_run)?;
    if options.json {
        print_json(&report)?;
    } else {
        status::print_adopt_report(&report);
    }
    Ok(())
}

fn run_resolve(options: &GlobalOptions, command: ResolveCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    match command.command {
        ResolveSubcommand::List => {
            let report = adopt::list_resolve_items(&paths)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_resolve_list_report(&report);
            }
            Ok(())
        }
        ResolveSubcommand::Adopt(args) => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = adopt::adopt_skill(&paths, &args.id, options.yes, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_adopt_report(&report);
            }
            Ok(())
        }
        ResolveSubcommand::Keep(args) => {
            let _lock = store::StoreLock::acquire(&paths)?;
            let report = adopt::keep_unmanaged_skill(&paths, &args.id)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_keep_report(&report);
            }
            Ok(())
        }
        ResolveSubcommand::RemoveOwned(args) => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = adopt::remove_owned_skill(&paths, &args.id, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_remove_owned_report(&report);
            }
            Ok(())
        }
    }
}

fn run_doctor(options: &GlobalOptions) -> DaloResult<()> {
    let report = doctor::run_doctor(&options.store);
    if options.json {
        print_json(&report)?;
    } else {
        status::print_doctor_report(&report);
    }
    Ok(())
}

fn run_target(options: &GlobalOptions, command: TargetCommand) -> DaloResult<()> {
    match command.command {
        TargetSubcommand::Detect => {
            let report = target::detect_targets(&options.store)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_target_detect_report(&report);
            }
            Ok(())
        }
        TargetSubcommand::Link(args) => {
            let paths = store::StorePaths::new(options.store.clone());
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = target::link_target(
                &options.store,
                &args.target,
                args.path.as_deref(),
                options.dry_run,
            )?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_target_link_report(&report);
            }
            Ok(())
        }
        TargetSubcommand::Unlink(args) => {
            let paths = store::StorePaths::new(options.store.clone());
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = target::unlink_target(&options.store, &args.target, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_target_unlink_report(&report);
            }
            Ok(())
        }
    }
}

fn print_json<T>(value: &T) -> DaloResult<()>
where
    T: serde::Serialize,
{
    serde_json::to_writer_pretty(std::io::stdout(), value)?;
    println!();
    Ok(())
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
            command: None,
        };

        assert!(run_cli(cli).is_ok());
    }
}
