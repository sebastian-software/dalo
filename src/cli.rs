//! Command-line parser and handlers.

use std::fs;
use std::io;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand};
use clap_complete::{Shell, generate};
use clap_mangen::Man;

use crate::adopt;
use crate::approval;
use crate::catalog;
use crate::doctor;
use crate::error::{DaloError, DaloResult};
use crate::instructions;
use crate::lockfile;
use crate::materialize;
use crate::resolver;
use crate::source;
use crate::status;
use crate::store;
use crate::target;

/// Parsed command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "dalo")]
#[command(
    version,
    about = "Git-backed skill management for AI agents.",
    long_about = "Git-backed skill management for AI agents.\n\nDalo keeps a local store of skill sources, resolves one approved skill set, and links that set into the folders your agents already read.",
    after_help = "Start here: dalo init -> dalo target link <agent> -> dalo source add <id> <git-url> -> dalo sync\nTry safely: use --store with a temporary directory and target link generic <path>.",
    after_long_help = "Mental model:\n  store   local database under ~/.dalo, or --store PATH\n  source  Git-backed skill collection, including the built-in local source\n  sync    refreshes clean tracking sources, resolves approved skills, and links them into targets\n\nQuickstart:\n  1. dalo init\n  2. dalo target link <codex|claude|openclaw|hermes>\n  3. dalo source add <id> <git-url>\n  4. dalo sync\n\nSafe sandbox:\n  export DALO_STORE=\"$(mktemp -d)/store\"\n  dalo init\n  dalo target link generic \"$(mktemp -d)/skills\""
)]
pub struct Cli {
    /// Override the dalo store path.
    #[arg(long, global = true, value_name = "PATH")]
    pub store: Option<PathBuf>,

    /// Emit machine-readable JSON where supported.
    #[arg(long, global = true)]
    pub json: bool,

    /// Reserved for future safe interactive prompts; currently a no-op.
    #[arg(long, global = true, hide = true)]
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
    /// Reserved for future safe prompts.
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
    Status(CheckArgs),
    /// Refresh clean sources, resolve approved skills, and link them into targets.
    #[command(
        long_about = "Refresh clean tracking sources, resolve the approved skill set, and materialize it into linked target folders.\n\nA skill source is a Git-backed collection of skills. Sync never overwrites unmanaged files; blocked or shadowed skills are reported instead."
    )]
    Sync,
    /// Adopt an unmanaged skill into the local source.
    Adopt(AdoptCommand),
    /// Run explicit safe repair helpers.
    Resolve(ResolveCommand),
    /// Diagnose store, target, Git, and lockfile health.
    Doctor(CheckArgs),
    /// Grant, list, and revoke source-qualified skill approvals.
    Approve(ApproveCommand),
    /// Manage instruction packs rendered into instruction files.
    Instructions(InstructionsCommand),
    /// Generate shell completions.
    #[command(hide = true)]
    Completions(CompletionsCommand),
    /// Generate a man page.
    #[command(hide = true)]
    Manpage,
}

/// `completions` command.
#[derive(Debug, Args)]
pub struct CompletionsCommand {
    /// Shell to generate completions for.
    pub shell: Shell,
}

/// `instructions` command group.
#[derive(Debug, Args)]
pub struct InstructionsCommand {
    /// Instructions subcommand.
    #[command(subcommand)]
    pub command: InstructionsSubcommand,
}

/// `instructions` subcommands.
#[derive(Debug, Subcommand)]
pub enum InstructionsSubcommand {
    /// Render a local instruction pack into a target file as a managed block.
    Enable(InstructionsFileArgs),
    /// Remove a pack's managed block from a target file.
    Disable(InstructionsFileArgs),
    /// List active instruction packs recorded in the user lock.
    List,
}

/// Arguments for `instructions enable`/`disable`.
#[derive(Debug, Args)]
pub struct InstructionsFileArgs {
    /// Instruction pack ID (a `local/instructions/<id>.md` file).
    pub pack: String,

    /// Target instruction file to render into.
    pub file: PathBuf,
}

/// Optional automation check behavior for report commands.
#[derive(Debug, Args)]
pub struct CheckArgs {
    /// Exit non-zero when the report contains a state requiring review.
    #[arg(long)]
    pub check: bool,
}

/// `approve` command group.
#[derive(Debug, Args)]
pub struct ApproveCommand {
    /// Approval subcommand.
    #[command(subcommand)]
    pub command: ApproveSubcommand,
}

/// Approval lifecycle subcommands.
#[derive(Debug, Subcommand)]
pub enum ApproveSubcommand {
    /// List local approval records.
    List,
    /// Approve one source-qualified skill.
    Skill(ApprovalValueArgs),
    /// Trust every skill from one configured source.
    Source(ApprovalValueArgs),
    /// Trust skills owned by one source-qualified author.
    Author(ApprovalValueArgs),
    /// Trust skills owned by one source-qualified organization.
    Org(ApprovalValueArgs),
    /// Revoke one exact source-qualified approval.
    Revoke(ApprovalRevokeArgs),
}

/// One approval value.
#[derive(Debug, Args)]
pub struct ApprovalValueArgs {
    /// Source-qualified approval value.
    pub value: String,
}

/// One approval to revoke.
#[derive(Debug, Args)]
pub struct ApprovalRevokeArgs {
    /// Approval scope: skill, source, author, or org.
    pub scope: String,
    /// Source-qualified approval value.
    pub value: String,
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
    /// Add a catalog source (a multi-skill repository) from a Git URL.
    AddCatalog(SourceAddArgs),
    /// List configured sources.
    List,
    /// Change a source priority.
    Priority(SourcePriorityArgs),
    /// Inspect a catalog source's available skills.
    Inspect(SourceInspectArgs),
    /// Select or unselect catalog skills.
    Select(SourceSelectArgs),
    /// Check a catalog source for upstream drift (read-only).
    Refresh(SourceRefreshArgs),
    /// Remove a team or catalog source and reconcile its owned links.
    Remove(SourceRemoveArgs),
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

/// Arguments for `source inspect`.
#[derive(Debug, Args)]
pub struct SourceInspectArgs {
    /// Catalog source ID.
    pub id: String,
}

/// Arguments for `source select`.
#[derive(Debug, Args)]
pub struct SourceSelectArgs {
    /// Catalog source ID.
    pub id: String,

    /// Skill references to select (stable ID or slot name).
    #[arg(required = true)]
    pub skills: Vec<String>,

    /// Unselect the given skills instead of selecting them.
    #[arg(long)]
    pub unselect: bool,
}

/// Arguments for `source refresh`.
#[derive(Debug, Args)]
pub struct SourceRefreshArgs {
    /// Catalog source ID.
    pub id: String,

    /// Check for drift without advancing the pin.
    #[arg(long)]
    pub check: bool,
}

/// Arguments for `source remove`.
#[derive(Debug, Args)]
pub struct SourceRemoveArgs {
    /// Team or catalog source ID.
    pub id: String,
    /// Retain the Git checkout after removing all Dalo state for the source.
    #[arg(long)]
    pub keep_checkout: bool,
}

/// Arguments for `adopt`.
#[derive(Debug, Args)]
pub struct AdoptCommand {
    /// Skill slot name or path to adopt.
    pub skill: String,

    /// Replace the original unmanaged folder with an owned symlink after copying.
    #[arg(long)]
    pub replace: bool,
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
    Adopt(ResolveAdoptArgs),
    /// Keep and protect the referenced unmanaged entry.
    Keep(ResolveIdArg),
    /// Remove an owned symlink by recorded ID.
    RemoveOwned(ResolveIdArg),
}

/// Arguments for `resolve adopt`.
#[derive(Debug, Args)]
pub struct ResolveAdoptArgs {
    /// Diagnostic or state item ID.
    pub id: String,

    /// Replace the original unmanaged folder with an owned symlink after copying.
    #[arg(long)]
    pub replace: bool,
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

    match command {
        Command::Completions(command) => return run_completions(command),
        Command::Manpage => return run_manpage(),
        _ => {}
    }

    let options = GlobalOptions::resolve(store.as_deref(), json, yes, dry_run)?;

    match command {
        Command::Init => run_init(&options),
        Command::Target(command) => run_target(&options, command),
        Command::Source(command) => run_source(&options, command),
        Command::Status(args) => run_status(&options, args),
        Command::Sync => run_sync(&options),
        Command::Adopt(command) => run_adopt(&options, command),
        Command::Resolve(command) => run_resolve(&options, command),
        Command::Doctor(args) => run_doctor(&options, args),
        Command::Approve(command) => run_approve(&options, command),
        Command::Instructions(command) => run_instructions(&options, command),
        Command::Completions(_) | Command::Manpage => {
            unreachable!("handled before store resolution")
        }
    }
}

fn run_completions(command: CompletionsCommand) -> DaloResult<()> {
    let mut clap_command = Cli::command();
    generate(command.shell, &mut clap_command, "dalo", &mut io::stdout());
    Ok(())
}

fn run_manpage() -> DaloResult<()> {
    let mut buffer = Vec::new();
    Man::new(Cli::command()).render(&mut buffer)?;
    io::copy(&mut buffer.as_slice(), &mut io::stdout())?;
    Ok(())
}

fn run_instructions(options: &GlobalOptions, command: InstructionsCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    match command.command {
        InstructionsSubcommand::Enable(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report =
                instructions::enable_pack(&paths, &args.pack, &args.file, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_instruction_pack_report(&report);
            }
            Ok(())
        }
        InstructionsSubcommand::Disable(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report =
                instructions::disable_pack(&paths, &args.pack, &args.file, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_instruction_pack_report(&report);
            }
            Ok(())
        }
        InstructionsSubcommand::List => {
            let lock = store::read_user_lock(&paths)?;
            if options.json {
                print_json(&lock.active_instruction_packs)?;
            } else if lock.active_instruction_packs.is_empty() {
                println!("no active instruction packs");
            } else {
                for pack in &lock.active_instruction_packs {
                    println!("{} -> {}", pack.pack_id, pack.target.display());
                }
            }
            Ok(())
        }
    }
}

fn run_init(options: &GlobalOptions) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    let _lock = if options.dry_run {
        None
    } else {
        fs::create_dir_all(&paths.root)?;
        Some(store::StoreLock::acquire(&paths)?)
    };
    let report = store::init_store(options.store.clone(), options.dry_run)?;

    if options.json {
        print_json(&report)?;
    } else {
        status::print_init_report(&report);
    }

    Ok(())
}

fn run_status(options: &GlobalOptions, args: CheckArgs) -> DaloResult<()> {
    let report = status::build_status_report(&options.store)?;

    if options.json {
        print_json(&report)?;
    } else {
        status::print_status_report(&report);
    }

    if args.check && status_requires_review(&report) {
        return Err(DaloError::CheckFailed {
            reason: "status reports unresolved drift, pending approvals, or blocked state"
                .to_owned(),
        });
    }
    Ok(())
}

fn status_requires_review(report: &status::StatusReport) -> bool {
    !report.inventory_warnings.is_empty()
        || report.sources.iter().any(|source| source.error.is_some())
        || !report.resolution.pending_approval_skills.is_empty()
        || !report.resolution.blocked_skills.is_empty()
        || !report.lock.drift.is_empty()
        || !report.unmanaged_skills.is_empty()
        || !report.instruction_block_drifts.is_empty()
}

fn run_sync(options: &GlobalOptions) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    // Read the existing lock before mutating sources or targets. A malformed or
    // newer lock is recovery data, not an empty baseline we are allowed to
    // overwrite after a successful materialization pass.
    let previous = if options.dry_run {
        None
    } else {
        Some(store::read_user_lock(&paths)?)
    };
    let config = store::read_config(&paths)?;
    if !options.dry_run {
        source::refresh_tracking_team_sources_from_config(&config)?;
    }
    let approvals = store::read_approvals(&paths)?;
    let live = resolver::resolve_from_config(&config, approvals.approvals);
    let degraded_sources = live
        .scans
        .iter()
        .filter(|scan| {
            scan.error.is_some()
                || scan
                    .inventory
                    .as_ref()
                    .is_some_and(resolver::inventory_degrades_source_for_removal)
        })
        .map(|scan| materialize::DegradedSource {
            id: scan.source.id.clone(),
            path: scan.source.path.clone(),
            reason: scan
                .error
                .clone()
                .unwrap_or_else(|| "inventory warnings make removals unsafe".to_owned()),
        })
        .collect::<Vec<_>>();
    let (report, rollback) = materialize::materialize_with_degraded_sources_rollback(
        &paths,
        &live.resolution,
        options.dry_run,
        &degraded_sources,
    )?;
    if !options.dry_run {
        let mut lock = lockfile::build_user_lock(&config.sources, &live.resolution, Some(&report));
        // Instruction packs are owned by the `instructions` command; preserve them
        // across a sync instead of dropping them.
        lock.active_instruction_packs = previous
            .expect("non-dry-run sync reads the user lock before materializing")
            .active_instruction_packs;
        if let Err(error) = store::write_user_lock(&paths, &lock) {
            if let Some(rollback) = rollback
                && let Err(rollback_error) = rollback.restore(&paths)
            {
                return Err(DaloError::Io(std::io::Error::other(format!(
                    "{error}; additionally failed to roll back sync: {rollback_error}"
                ))));
            }
            return Err(error);
        }
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
            ensure_initialized(&paths)?;
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
        SourceSubcommand::AddCatalog(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let source =
                catalog::add_catalog_source(&paths, &args.id, &args.location, options.dry_run)?;
            if options.json {
                print_json(&source)?;
            } else {
                status::print_catalog_add_report(&source, options.dry_run);
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
        SourceSubcommand::Inspect(args) => {
            let report = catalog::inspect_catalog(&paths, &args.id)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_catalog_inspect_report(&report);
            }
            Ok(())
        }
        SourceSubcommand::Select(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = catalog::select_skills(
                &paths,
                &args.id,
                &args.skills,
                args.unselect,
                options.dry_run,
            )?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_catalog_select_report(&report);
            }
            Ok(())
        }
        SourceSubcommand::Refresh(args) => {
            ensure_initialized(&paths)?;
            let _lock = store::StoreLock::acquire(&paths)?;
            let report = catalog::check_catalog_drift(&paths, &args.id)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_catalog_drift_report(&report);
            }
            if args.check
                && report
                    .outcomes
                    .iter()
                    .any(|outcome| outcome.code != catalog::DriftCode::NewAvailable)
            {
                return Err(DaloError::CheckFailed {
                    reason: "selected catalog skills changed, moved, or were removed upstream"
                        .to_owned(),
                });
            }
            Ok(())
        }
        SourceSubcommand::Remove(args) => run_source_remove(options, &paths, args),
        SourceSubcommand::Priority(args) => {
            ensure_initialized(&paths)?;
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

fn run_source_remove(
    options: &GlobalOptions,
    paths: &store::StorePaths,
    args: SourceRemoveArgs,
) -> DaloResult<()> {
    ensure_initialized(paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(paths)?)
    };
    let mut plan =
        source::plan_remove_source(paths, &args.id, args.keep_checkout, options.dry_run)?;
    if options.dry_run {
        let live = resolver::resolve_from_config(&plan.config, plan.approvals.approvals.clone());
        let (materialization, _) = materialize::materialize_with_degraded_sources_rollback(
            paths,
            &live.resolution,
            true,
            &[],
        )?;
        plan.report.reconciled_links = materialization
            .operations
            .iter()
            .filter(|operation| {
                matches!(
                    operation.kind,
                    materialize::MaterializeOperationKind::Create
                        | materialize::MaterializeOperationKind::Relink
                        | materialize::MaterializeOperationKind::Remove
                        | materialize::MaterializeOperationKind::DropRecord
                )
            })
            .map(|operation| operation.link_path.clone())
            .collect();
        if options.json {
            print_json(&plan.report)?;
        } else {
            status::print_source_remove_report(&plan.report);
        }
        return Ok(());
    }

    let previous_user_lock = store::read_user_lock(paths)?;
    let live = resolver::resolve_from_config(&plan.config, plan.approvals.approvals.clone());
    let (materialization, rollback) = materialize::materialize_with_degraded_sources_rollback(
        paths,
        &live.resolution,
        false,
        &[],
    )?;
    plan.report.reconciled_links = materialization
        .operations
        .iter()
        .filter(|operation| {
            matches!(
                operation.kind,
                materialize::MaterializeOperationKind::Create
                    | materialize::MaterializeOperationKind::Relink
                    | materialize::MaterializeOperationKind::Remove
                    | materialize::MaterializeOperationKind::DropRecord
            )
        })
        .map(|operation| operation.link_path.clone())
        .collect();
    let mut user_lock = lockfile::build_user_lock(
        &plan.config.sources,
        &live.resolution,
        Some(&materialization),
    );
    user_lock.active_instruction_packs = previous_user_lock.active_instruction_packs.clone();

    let staged_checkout = if args.keep_checkout || !plan.report.checkout_path.exists() {
        None
    } else {
        let staged = plan
            .report
            .checkout_path
            .with_file_name("checkout.dalo-removing");
        if staged.exists() {
            return rollback_remove(
                paths,
                &plan,
                &previous_user_lock,
                rollback,
                None,
                DaloError::InvalidStorePath {
                    path: staged,
                    reason: "stale source-removal staging path exists; inspect it before retrying"
                        .to_owned(),
                },
            );
        }
        Some(staged)
    };

    let commit = (|| -> DaloResult<()> {
        if let Some(staged) = &staged_checkout {
            source_remove_boundary("stage_checkout")?;
            std::fs::rename(&plan.report.checkout_path, staged)?;
        }
        source_remove_boundary("config")?;
        store::write_config(paths, &plan.config)?;
        source_remove_boundary("source_lock")?;
        catalog::write_source_lock(paths, &plan.source_lock)?;
        source_remove_boundary("approvals")?;
        store::write_approvals(paths, &plan.approvals)?;
        source_remove_boundary("user_lock")?;
        store::write_user_lock(paths, &user_lock)?;
        Ok(())
    })();
    if let Err(error) = commit {
        return rollback_remove(
            paths,
            &plan,
            &previous_user_lock,
            rollback,
            staged_checkout.as_deref(),
            error,
        );
    }
    if let Some(staged) = staged_checkout {
        let cleanup = (|| -> DaloResult<()> {
            source_remove_boundary("checkout_cleanup")?;
            std::fs::remove_dir_all(&staged)?;
            Ok(())
        })();
        if let Err(error) = cleanup {
            return rollback_remove(
                paths,
                &plan,
                &previous_user_lock,
                rollback,
                Some(&staged),
                error,
            );
        }
    }

    if options.json {
        print_json(&plan.report)?;
    } else {
        status::print_source_remove_report(&plan.report);
    }
    Ok(())
}

fn rollback_remove(
    paths: &store::StorePaths,
    plan: &source::SourceRemovalPlan,
    original_user_lock: &lockfile::UserLock,
    rollback: Option<materialize::MaterializationRollback>,
    staged_checkout: Option<&std::path::Path>,
    error: DaloError,
) -> DaloResult<()> {
    let mut rollback_errors = Vec::new();
    if let Some(staged) = staged_checkout
        && staged.exists()
        && let Err(restore_error) = std::fs::rename(staged, &plan.report.checkout_path)
    {
        rollback_errors.push(restore_error.to_string());
    }
    if let Err(restore_error) = store::write_config(paths, &plan.original_config) {
        rollback_errors.push(restore_error.to_string());
    }
    if let Err(restore_error) = catalog::write_source_lock(paths, &plan.original_source_lock) {
        rollback_errors.push(restore_error.to_string());
    }
    if let Err(restore_error) = store::write_approvals(paths, &plan.original_approvals) {
        rollback_errors.push(restore_error.to_string());
    }
    if let Err(restore_error) = store::write_user_lock(paths, original_user_lock) {
        rollback_errors.push(restore_error.to_string());
    }
    if let Some(rollback) = rollback
        && let Err(restore_error) = rollback.restore(paths)
    {
        rollback_errors.push(restore_error.to_string());
    }
    if rollback_errors.is_empty() {
        Err(error)
    } else {
        Err(DaloError::Io(std::io::Error::other(format!(
            "{error}; additionally failed to roll back source removal: {}",
            rollback_errors.join("; ")
        ))))
    }
}

/// Trigger a named source-removal failpoint for integration tests.
///
/// The hook exists solely to exercise every transaction boundary. It is inert
/// unless the test-only environment variable names the exact boundary.
fn source_remove_boundary(boundary: &str) -> DaloResult<()> {
    if std::env::var("DALO_SOURCE_REMOVE_FAIL_AT").ok().as_deref() == Some(boundary) {
        return Err(DaloError::CheckFailed {
            reason: format!("injected source-removal failure at {boundary}"),
        });
    }
    Ok(())
}

fn run_adopt(options: &GlobalOptions, command: AdoptCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    let report = adopt::adopt_skill(&paths, &command.skill, command.replace, options.dry_run)?;
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
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = adopt::adopt_skill(&paths, &args.id, args.replace, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_adopt_report(&report);
            }
            Ok(())
        }
        ResolveSubcommand::Keep(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = adopt::keep_unmanaged_skill(&paths, &args.id, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_keep_report(&report);
            }
            Ok(())
        }
        ResolveSubcommand::RemoveOwned(args) => {
            ensure_initialized(&paths)?;
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

fn run_doctor(options: &GlobalOptions, args: CheckArgs) -> DaloResult<()> {
    let report = doctor::run_doctor(&options.store);
    if options.json {
        print_json(&report)?;
    } else {
        status::print_doctor_report(&report);
    }
    if args.check && report.summary.errors > 0 {
        return Err(DaloError::CheckFailed {
            reason: format!("doctor found {} error findings", report.summary.errors),
        });
    }
    Ok(())
}

fn run_approve(options: &GlobalOptions, command: ApproveCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    match command.command {
        ApproveSubcommand::List => {
            let report = approval::list(&paths)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_approval_list(&report);
            }
        }
        ApproveSubcommand::Skill(args) => print_approval_result(
            options,
            approval::grant(&paths, "skill", &args.value, options.dry_run)?,
        )?,
        ApproveSubcommand::Source(args) => print_approval_result(
            options,
            approval::grant(&paths, "source", &args.value, options.dry_run)?,
        )?,
        ApproveSubcommand::Author(args) => print_approval_result(
            options,
            approval::grant(&paths, "author", &args.value, options.dry_run)?,
        )?,
        ApproveSubcommand::Org(args) => print_approval_result(
            options,
            approval::grant(&paths, "org", &args.value, options.dry_run)?,
        )?,
        ApproveSubcommand::Revoke(args) => print_approval_result(
            options,
            approval::revoke(&paths, &args.scope, &args.value, options.dry_run)?,
        )?,
    }
    Ok(())
}

fn print_approval_result(
    options: &GlobalOptions,
    report: approval::ApprovalReport,
) -> DaloResult<()> {
    if options.json {
        print_json(&report)
    } else {
        status::print_approval_report(&report);
        Ok(())
    }
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
            ensure_initialized(&paths)?;
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
            ensure_initialized(&paths)?;
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

fn ensure_initialized(paths: &store::StorePaths) -> DaloResult<()> {
    if !paths.config_file.exists() {
        return Err(DaloError::StoreNotInitialized {
            path: paths.root.clone(),
        });
    }
    Ok(())
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
