//! Command-line parser and handlers.

use std::fs;
use std::io;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use clap_mangen::Man;

use crate::adopt;
use crate::approval;
use crate::audit;
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
use crate::update;

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
    Sync(CheckArgs),
    /// Adopt an unmanaged skill into the local source.
    #[command(
        after_help = "Examples:\n  dalo adopt review-helper\n  dalo adopt review-helper --replace\n  dalo --dry-run adopt /path/to/target/review-helper"
    )]
    Adopt(AdoptCommand),
    /// Run explicit safe repair helpers.
    #[command(
        after_help = "Examples:\n  dalo resolve list\n  dalo resolve adopt review-helper --replace\n  dalo resolve keep review-helper\n  dalo resolve unkeep claude:review-helper\n  dalo resolve remove-owned claude:review-helper"
    )]
    Resolve(ResolveCommand),
    /// Diagnose store, target, Git, and lockfile health.
    Doctor(CheckArgs),
    /// Inspect a skill with deterministic checks and an optional isolated AI reviewer.
    #[command(
        after_help = "Examples:\n  dalo audit public:review-helper\n  dalo audit ./my-skill --agent auto\n  dalo audit public:review-helper --agent codex --check\n  dalo audit public:review-helper --accept-risk 'reviewed upstream installer'"
    )]
    Audit(AuditCommand),
    /// Grant, list, and revoke scoped approval records.
    #[command(
        after_help = "Examples:\n  dalo approve list\n  dalo approve skill public:review-helper\n  dalo approve source team\n  dalo approve author public:maintainers\n  dalo approve org public:example-org\n  dalo approve revoke skill public:review-helper"
    )]
    Approve(ApproveCommand),
    /// Manage instruction packs rendered into instruction files.
    Instructions(InstructionsCommand),
    /// Generate shell completions.
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
    Skill(SkillApprovalArgs),
    /// Trust every skill from one configured source.
    Source(SourceApprovalArgs),
    /// Trust skills owned by one source-qualified author.
    Author(AuthorApprovalArgs),
    /// Trust skills owned by one source-qualified organization.
    Org(OrgApprovalArgs),
    /// Revoke one exact source-qualified approval.
    Revoke(ApprovalRevokeArgs),
}

/// One skill approval value.
#[derive(Debug, Args)]
pub struct SkillApprovalArgs {
    /// Skill in `<source>:<slot>` format, for example `public:review-helper`.
    #[arg(value_name = "VALUE")]
    pub value: String,

    /// Run an isolated semantic review; this may send skill contents to its provider.
    #[arg(long, value_enum, default_value_t = AuditAgentArg::None)]
    pub agent: AuditAgentArg,

    /// Ignore a compatible cached semantic review.
    #[arg(long)]
    pub refresh_audit: bool,

    /// Accept blocking findings for this exact content hash with a reason.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
}

/// Arguments for `audit`.
#[derive(Debug, Args)]
pub struct AuditCommand {
    /// Existing skill path or source-qualified `<source>:<skill>` reference.
    pub target: String,

    /// Semantic reviewer; this may send skill contents to its configured provider.
    #[arg(long, value_enum, default_value_t = AuditAgentArg::None)]
    pub agent: AuditAgentArg,

    /// Ignore a compatible cached semantic review.
    #[arg(long = "refresh-audit", alias = "refresh")]
    pub refresh_audit: bool,

    /// Exit non-zero when unaccepted high or critical findings exist.
    #[arg(long)]
    pub check: bool,

    /// Accept blocking findings for this exact content hash with a reason.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
}

/// Agent provider selection exposed by audit-related commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuditAgentArg {
    /// Deterministic local analysis only.
    None,
    /// First available provider with enforceable no-tool mode (Claude or OpenCode).
    Auto,
    /// OpenAI Codex CLI.
    Codex,
    /// Anthropic Claude Code CLI.
    Claude,
    /// OpenCode CLI.
    Opencode,
}

impl From<AuditAgentArg> for audit::AgentSelection {
    fn from(value: AuditAgentArg) -> Self {
        match value {
            AuditAgentArg::None => Self::None,
            AuditAgentArg::Auto => Self::Auto,
            AuditAgentArg::Codex => Self::Provider(audit::AgentProvider::Codex),
            AuditAgentArg::Claude => Self::Provider(audit::AgentProvider::Claude),
            AuditAgentArg::Opencode => Self::Provider(audit::AgentProvider::Opencode),
        }
    }
}

/// One source approval value.
#[derive(Debug, Args)]
pub struct SourceApprovalArgs {
    /// Configured source ID, for example `team`.
    #[arg(value_name = "VALUE")]
    pub value: String,
}

/// One author approval value.
#[derive(Debug, Args)]
pub struct AuthorApprovalArgs {
    /// Author in `<source>:<owner>` format, for example `public:maintainers`.
    #[arg(value_name = "VALUE")]
    pub value: String,
}

/// One organization approval value.
#[derive(Debug, Args)]
pub struct OrgApprovalArgs {
    /// Organization in `<source>:<owner>` format, for example `public:example-org`.
    #[arg(value_name = "VALUE")]
    pub value: String,
}

/// One approval to revoke.
#[derive(Debug, Args)]
pub struct ApprovalRevokeArgs {
    /// Approval scope: skill, source, author, or org.
    pub scope: String,
    /// Approval value in the format required by the selected scope.
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
    AddCatalog(SourceAddCatalogArgs),
    /// List configured sources.
    List,
    /// Change a source priority.
    Priority(SourcePriorityArgs),
    /// Inspect a catalog source's available skills.
    Inspect(SourceInspectArgs),
    /// Select or unselect catalog skills.
    #[command(
        after_help = "Examples:\n  dalo source select public review-helper\n  dalo source select public review-helper formatter\n  dalo source select public --unselect formatter\n  dalo --dry-run source select public review-helper"
    )]
    Select(SourceSelectArgs),
    /// Check a catalog source for upstream drift (read-only).
    Refresh(SourceRefreshArgs),
    /// Remove a team or catalog source and reconcile its owned links.
    #[command(
        after_help = "Examples:\n  dalo --dry-run --json source remove platform\n  dalo source remove public\n  dalo source remove public --keep-checkout"
    )]
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

/// Arguments for `source add-catalog`.
#[derive(Debug, Args)]
pub struct SourceAddCatalogArgs {
    /// Source ID.
    pub id: String,

    /// Git URL of the catalog source.
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

    /// Exit non-zero when selected skills drifted upstream.
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

    /// Run a semantic review; this may send skill contents to its provider.
    #[arg(long, value_enum, default_value_t = AuditAgentArg::None)]
    pub agent: AuditAgentArg,

    /// Ignore a compatible cached semantic review.
    #[arg(long)]
    pub refresh_audit: bool,

    /// Accept blocking findings for this exact content hash with a reason.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
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
    /// Keep an unmanaged entry in place and treat its sync conflict as non-failing.
    Keep(ResolveIdArg),
    /// Remove protection from a target slot.
    Unkeep(ResolveIdArg),
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

    /// Run a semantic review; this may send skill contents to its provider.
    #[arg(long, value_enum, default_value_t = AuditAgentArg::None)]
    pub agent: AuditAgentArg,

    /// Ignore a compatible cached semantic review.
    #[arg(long)]
    pub refresh_audit: bool,

    /// Accept blocking findings for this exact content hash with a reason.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
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
        Command::Completions(command) => {
            warn_noop_dry_run(dry_run, json);
            return run_completions(command);
        }
        Command::Manpage => {
            warn_noop_dry_run(dry_run, json);
            return run_manpage();
        }
        _ => {}
    }

    let options = GlobalOptions::resolve(store.as_deref(), json, yes, dry_run)?;
    if command_ignores_dry_run(&command) {
        warn_noop_dry_run(options.dry_run, options.json);
    }

    let result = match command {
        Command::Init => run_init(&options),
        Command::Target(command) => run_target(&options, command),
        Command::Source(command) => run_source(&options, command),
        Command::Status(args) => run_status(&options, args),
        Command::Sync(args) => run_sync(&options, args),
        Command::Adopt(command) => run_adopt(&options, command),
        Command::Resolve(command) => run_resolve(&options, command),
        Command::Doctor(args) => run_doctor(&options, args),
        Command::Audit(command) => run_audit(&options, command),
        Command::Approve(command) => run_approve(&options, command),
        Command::Instructions(command) => run_instructions(&options, command),
        Command::Completions(_) | Command::Manpage => {
            unreachable!("handled before store resolution")
        }
    };

    if result.is_ok() && !options.json {
        update::maybe_print_notice();
    }

    result
}

fn warn_noop_dry_run(dry_run: bool, json: bool) {
    if dry_run && !json {
        eprintln!("note: --dry-run has no effect for this read-only command");
    }
}

fn command_ignores_dry_run(command: &Command) -> bool {
    matches!(
        command,
        Command::Target(TargetCommand {
            command: TargetSubcommand::Detect
        }) | Command::Source(SourceCommand {
            command: SourceSubcommand::List
                | SourceSubcommand::Inspect(_)
                | SourceSubcommand::Refresh(_)
        }) | Command::Status(_)
            | Command::Doctor(_)
            | Command::Approve(ApproveCommand {
                command: ApproveSubcommand::List
            })
            | Command::Instructions(InstructionsCommand {
                command: InstructionsSubcommand::List
            })
    )
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
    let paths = store::StorePaths::new(options.store.clone());
    let _lock = if args.check && paths.config_file.exists() {
        Some(store::StoreLock::acquire(&paths)?)
    } else {
        None
    };
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
        || !report.blocking_audits.is_empty()
        || report
            .materialization
            .iter()
            .any(|operation| operation.status == materialize::MaterializeOperationStatus::Blocked)
        || report
            .resolution
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code.requires_review())
        || !report.lock.drift.is_empty()
        || (report.targets.is_empty() && !report.resolution.active_skills.is_empty())
        || report.unmanaged_skills.iter().any(|skill| !skill.protected)
        || !report.instruction_block_drifts.is_empty()
}

fn run_sync(options: &GlobalOptions, args: CheckArgs) -> DaloResult<()> {
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
    let refresh_failures = if options.dry_run {
        Vec::new()
    } else {
        source::refresh_tracking_team_sources_from_config(&paths, &config)?
    };
    let approvals = store::read_approvals(&paths)?;
    let live = resolver::resolve_from_config(&config, approvals.approvals);
    let blocking_audits = audit_active_skills(&paths, &live.resolution, options.dry_run)?;
    if !blocking_audits.is_empty() {
        return Err(DaloError::AuditBlocked {
            reason: format!(
                "security audit blocked {} skill{} ({}); inspect with `dalo audit <source:skill>` or record an explicit `--accept-risk` reason",
                blocking_audits.len(),
                if blocking_audits.len() == 1 { "" } else { "s" },
                blocking_audits.join(", ")
            ),
        });
    }
    let degraded_sources = collect_degraded_sources(&live, refresh_failures);
    let (report, rollback) = materialize::materialize_with_degraded_sources_rollback(
        &paths,
        &live.resolution,
        options.dry_run,
        &degraded_sources,
    )?;
    if !options.dry_run {
        let previous = previous.expect("non-dry-run sync reads the user lock before materializing");
        let mut lock = lockfile::build_user_lock(&config.sources, &live.resolution, Some(&report));
        // Instruction packs are owned by the `instructions` command; preserve them
        // across a sync instead of dropping them.
        lock.active_instruction_packs = previous.active_instruction_packs.clone();
        write_sync_lock_with_rollback(&paths, &previous, &lock, rollback, store::write_user_lock)?;
    }

    if options.json {
        print_json(&report)?;
    } else {
        status::print_sync_report(&report);
    }

    if args.check
        && let Some(reason) = sync_review_reason(&report)
    {
        return Err(DaloError::CheckFailed { reason });
    }

    Ok(())
}

fn audit_active_skills(
    paths: &store::StorePaths,
    resolution: &resolver::Resolution,
    dry_run: bool,
) -> DaloResult<Vec<String>> {
    let mut blocked = Vec::new();
    for skill in &resolution.active_skills {
        let report = audit::audit_skill(
            paths,
            &skill.source_ref,
            &skill.path,
            &audit::AuditOptions {
                persist: !dry_run,
                ..audit::AuditOptions::default()
            },
        )?;
        if report.is_blocking() {
            blocked.push(skill.source_ref.clone());
        }
    }
    Ok(blocked)
}

fn collect_degraded_sources(
    live: &resolver::LiveResolution,
    refresh_failures: Vec<source::TrackingSourceRefreshFailure>,
) -> Vec<materialize::DegradedSource> {
    let mut degraded_sources = live
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
    for failure in refresh_failures {
        if let Some(existing) = degraded_sources
            .iter_mut()
            .find(|source| source.id == failure.id)
        {
            existing.reason = format!("{}; {}", existing.reason, failure.reason);
        } else {
            degraded_sources.push(materialize::DegradedSource {
                id: failure.id,
                path: failure.path,
                reason: failure.reason,
            });
        }
    }
    degraded_sources.sort_by(|left, right| left.id.cmp(&right.id));
    degraded_sources
}

fn write_sync_lock_with_rollback<F>(
    paths: &store::StorePaths,
    previous: &lockfile::UserLock,
    next: &lockfile::UserLock,
    rollback: Option<materialize::MaterializationRollback>,
    mut write_lock: F,
) -> DaloResult<()>
where
    F: FnMut(&store::StorePaths, &lockfile::UserLock) -> DaloResult<()>,
{
    let Err(error) = write_lock(paths, next) else {
        return Ok(());
    };

    let mut recovery_errors = Vec::new();
    if let Some(rollback) = rollback
        && let Err(rollback_error) = rollback.restore(paths)
    {
        recovery_errors.push(format!("roll back sync: {rollback_error}"));
    }
    if let Err(lock_error) = write_lock(paths, previous) {
        recovery_errors.push(format!("restore previous lock: {lock_error}"));
    }
    if recovery_errors.is_empty() {
        return Err(error);
    }
    Err(DaloError::Io(std::io::Error::other(format!(
        "{error}; additionally failed to {}",
        recovery_errors.join("; ")
    ))))
}

fn sync_review_reason(report: &materialize::SyncReport) -> Option<String> {
    let mut reasons = Vec::new();

    if report.linked_targets == 0 && !report.resolution.active_skills.is_empty() {
        let count = report.resolution.active_skills.len();
        reasons.push(format!(
            "{count} active skill{} but no linked targets",
            if count == 1 { "" } else { "s" }
        ));
    }

    let pending = &report.resolution.pending_approval_skills;
    if !pending.is_empty() {
        reasons.push(format!(
            "{} pending approval{} ({})",
            pending.len(),
            if pending.len() == 1 { "" } else { "s" },
            pending
                .iter()
                .map(|skill| skill.source_ref.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let blocked_skills = &report.resolution.blocked_skills;
    if !blocked_skills.is_empty() {
        reasons.push(format!(
            "{} blocked skill{} ({})",
            blocked_skills.len(),
            if blocked_skills.len() == 1 { "" } else { "s" },
            blocked_skills
                .iter()
                .map(|blocked| blocked.skill.source_ref.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let actionable_diagnostics = report
        .resolution
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code.requires_review())
        .filter(|diagnostic| {
            (pending.is_empty()
                || diagnostic.code != resolver::ResolutionDiagnosticCode::PendingApproval)
                && (blocked_skills.is_empty()
                    || diagnostic.code != resolver::ResolutionDiagnosticCode::RequiredBlocked)
        })
        .collect::<Vec<_>>();
    if !actionable_diagnostics.is_empty() {
        reasons.push(format!(
            "{} actionable diagnostic{} ({})",
            actionable_diagnostics.len(),
            if actionable_diagnostics.len() == 1 {
                ""
            } else {
                "s"
            },
            actionable_diagnostics
                .iter()
                .map(|diagnostic| diagnostic.source_ref.as_deref().map_or_else(
                    || resolver::diagnostic_code_name(diagnostic.code).to_owned(),
                    |source_ref| format!(
                        "{}:{source_ref}",
                        resolver::diagnostic_code_name(diagnostic.code)
                    )
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !report.degraded_sources.is_empty() {
        reasons.push(format!(
            "{} degraded source{} ({})",
            report.degraded_sources.len(),
            if report.degraded_sources.len() == 1 {
                ""
            } else {
                "s"
            },
            report
                .degraded_sources
                .iter()
                .map(|source| source.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let blocked_operations = report
        .operations
        .iter()
        .filter(|operation| operation.status == materialize::MaterializeOperationStatus::Blocked)
        .collect::<Vec<_>>();
    if !blocked_operations.is_empty() {
        reasons.push(format!(
            "{} blocked operation{} ({})",
            blocked_operations.len(),
            if blocked_operations.len() == 1 {
                ""
            } else {
                "s"
            },
            blocked_operations
                .iter()
                .map(|operation| operation.link_path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    (!reasons.is_empty()).then(|| reasons.join(", "))
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
            let location =
                source::resolve_source_location(&args.location, &std::env::current_dir()?);
            let report = source::add_team_source(&paths, &args.id, &location, options.dry_run)?;
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
            let location =
                source::resolve_source_location(&args.location, &std::env::current_dir()?);
            let source = catalog::add_catalog_source(&paths, &args.id, &location, options.dry_run)?;
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
    let previous_user_lock = store::read_user_lock(paths)?;
    let live = resolver::resolve_from_config(&plan.config, plan.approvals.approvals.clone());
    let degraded_sources = collect_degraded_sources(&live, Vec::new());
    let (materialization, rollback) = materialize::materialize_with_degraded_sources_rollback(
        paths,
        &live.resolution,
        options.dry_run,
        &degraded_sources,
    )?;
    populate_source_remove_report(&mut plan.report, &materialization, &previous_user_lock);

    if options.dry_run {
        if options.json {
            print_json(&plan.report)?;
        } else {
            status::print_source_remove_report(&plan.report);
        }
        return Ok(());
    }

    let mut user_lock = lockfile::build_user_lock(
        &plan.config.sources,
        &live.resolution,
        Some(&materialization),
    );
    user_lock.active_instruction_packs = previous_user_lock.active_instruction_packs.clone();

    let commit = (|| -> DaloResult<()> {
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
        return rollback_remove(paths, &plan, &previous_user_lock, rollback, error);
    }

    // Metadata and materialization are committed. Checkout deletion is garbage
    // collection from this point forward: failures are visible but must never
    // restore metadata that points at a partially deleted checkout.
    if !args.keep_checkout
        && let Err(error) = cleanup_removed_source_checkout(&plan.report.checkout_path)
    {
        plan.report.cleanup_warnings.push(format!(
            "checkout cleanup incomplete for `{}`: {error}",
            plan.report.checkout_path.display()
        ));
    }

    if options.json {
        print_json(&plan.report)?;
    } else {
        status::print_source_remove_report(&plan.report);
    }
    Ok(())
}

fn populate_source_remove_report(
    report: &mut source::SourceRemoveReport,
    materialization: &materialize::SyncReport,
    previous_user_lock: &lockfile::UserLock,
) {
    report.reconciled_links = materialization
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
        .map(|operation| source::SourceRemoveLink {
            kind: operation.kind,
            path: operation.link_path.clone(),
        })
        .collect();
    report.deactivated_skills = previous_user_lock
        .active_skills
        .iter()
        .filter(|skill| skill.source_id == report.source_id)
        .map(|skill| skill.source_ref.clone())
        .collect();
    report.deactivated_skills.sort();
    report.deactivated_skills.dedup();
}

fn cleanup_removed_source_checkout(checkout: &std::path::Path) -> DaloResult<()> {
    source_remove_boundary("stage_checkout")?;
    let source_dir = checkout
        .parent()
        .ok_or_else(|| DaloError::InvalidStorePath {
            path: checkout.to_path_buf(),
            reason: "source checkout has no parent directory".to_owned(),
        })?;
    cleanup_staged_source_audits(checkout, source_dir)?;
    let staged = checkout.with_file_name("checkout.dalo-removing");
    if staged.exists() {
        std::fs::remove_dir_all(&staged)?;
    }
    if checkout.exists() {
        std::fs::rename(checkout, &staged)?;
    }
    source_remove_boundary("checkout_cleanup")?;
    if source_dir.exists() {
        std::fs::remove_dir_all(source_dir)?;
    }
    Ok(())
}

fn cleanup_staged_source_audits(
    checkout: &std::path::Path,
    source_dir: &std::path::Path,
) -> DaloResult<()> {
    let Some(source_id) = source_dir.file_name() else {
        return Ok(());
    };
    let Some(sources_dir) = source_dir.parent() else {
        return Ok(());
    };
    let staging_root = sources_dir.join(".audit-staging");
    let Ok(entries) = std::fs::read_dir(&staging_root) else {
        return Ok(());
    };
    let prefix = format!("{}-", source_id.to_string_lossy());
    for entry in entries {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(&prefix) {
            continue;
        }
        let path = entry.path();
        if crate::git::remove_worktree(checkout, &path).is_err() {
            std::fs::remove_dir_all(path)?;
        }
    }
    crate::git::prune_worktrees(checkout)?;
    let _ = std::fs::remove_dir(staging_root);
    Ok(())
}

fn rollback_remove(
    paths: &store::StorePaths,
    plan: &source::SourceRemovalPlan,
    original_user_lock: &lockfile::UserLock,
    rollback: Option<materialize::MaterializationRollback>,
    error: DaloError,
) -> DaloResult<()> {
    let mut rollback_errors = Vec::new();
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
    run_adopt_with_audit(
        options,
        &paths,
        &command.skill,
        command.replace,
        command.agent,
        command.refresh_audit,
        command.accept_risk,
    )
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
            run_adopt_with_audit(
                options,
                &paths,
                &args.id,
                args.replace,
                args.agent,
                args.refresh_audit,
                args.accept_risk,
            )
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
        ResolveSubcommand::Unkeep(args) => {
            ensure_initialized(&paths)?;
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = adopt::unkeep_skill(&paths, &args.id, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_unkeep_report(&report);
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
    let paths = store::StorePaths::new(options.store.clone());
    let _lock = if args.check && paths.root.exists() {
        Some(store::StoreLock::acquire(&paths)?)
    } else {
        None
    };
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

fn run_audit(options: &GlobalOptions, command: AuditCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    let agent = prepare_agent_review(command.agent)?;
    let report = audit::audit_target(
        &paths,
        &command.target,
        &audit::AuditOptions {
            agent,
            refresh: command.refresh_audit,
            persist: !options.dry_run,
            accept_risk: command.accept_risk,
        },
    )?;
    if options.json {
        print_json(&report)?;
    } else {
        status::print_audit_report(&report);
    }
    if command.check && report.is_blocking() {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "security audit for `{}` contains unaccepted high or critical findings",
                report.source_ref
            ),
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
        ApproveSubcommand::Skill(args) => {
            let canonical = approval::canonical_skill(&paths, &args.value)?;
            let agent = prepare_agent_review(args.agent)?;
            let audit_report = audit::audit_target(
                &paths,
                &canonical,
                &audit::AuditOptions {
                    agent,
                    refresh: args.refresh_audit,
                    persist: !options.dry_run,
                    accept_risk: args.accept_risk,
                },
            )?;
            if audit_report.is_blocking() {
                if options.json {
                    print_json(&audit_report)?;
                } else {
                    status::print_audit_report(&audit_report);
                }
                return Err(DaloError::CheckFailed {
                    reason: format!(
                        "security audit blocked approval of `{}`; inspect the findings or rerun with `--accept-risk <reason>`",
                        audit_report.source_ref
                    ),
                });
            }
            let approval_report = approval::grant(&paths, "skill", &args.value, options.dry_run)?;
            if options.json {
                print_json(&SkillApprovalOutcome {
                    audit: audit_report,
                    approval: approval_report,
                })?;
            } else {
                status::print_audit_report(&audit_report);
                status::print_approval_report(&approval_report);
            }
        }
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

#[derive(serde::Serialize)]
struct SkillApprovalOutcome {
    audit: audit::AuditReport,
    approval: approval::ApprovalReport,
}

#[derive(serde::Serialize)]
struct AdoptAuditOutcome {
    audit: audit::AuditReport,
    adoption: adopt::AdoptReport,
}

#[allow(clippy::too_many_arguments)]
fn run_adopt_with_audit(
    options: &GlobalOptions,
    paths: &store::StorePaths,
    selector: &str,
    replace: bool,
    agent: AuditAgentArg,
    refresh: bool,
    accept_risk: Option<String>,
) -> DaloResult<()> {
    let unmanaged = adopt::find_unmanaged_skill(paths, selector)?;
    let agent = prepare_agent_review(agent)?;
    let audit_report = audit::audit_skill(
        paths,
        &format!("unmanaged:{}", unmanaged.slot_name),
        &unmanaged.path,
        &audit::AuditOptions {
            agent,
            refresh,
            persist: !options.dry_run,
            accept_risk,
        },
    )?;
    if audit_report.is_blocking() {
        if options.json {
            print_json(&audit_report)?;
        } else {
            status::print_audit_report(&audit_report);
        }
        return Err(DaloError::AuditBlocked {
            reason: format!(
                "refusing to adopt `{selector}` until its findings are reviewed or accepted with `--accept-risk <reason>`"
            ),
        });
    }
    let adoption = adopt::adopt_skill(paths, selector, replace, options.dry_run)?;
    if options.json {
        print_json(&AdoptAuditOutcome {
            audit: audit_report,
            adoption,
        })?;
    } else {
        status::print_audit_report(&audit_report);
        status::print_adopt_report(&adoption);
    }
    Ok(())
}

fn prepare_agent_review(agent: AuditAgentArg) -> DaloResult<audit::AgentSelection> {
    audit::resolve_agent_selection(agent.into())
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
    use std::cell::Cell;

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

    #[test]
    fn sync_lock_failure_should_restore_the_previous_lock() {
        let temp_dir = tempfile::tempdir().expect("tempdir should be created");
        let store_root = temp_dir.path().join("store");
        store::init_store(store_root.clone(), false).expect("init should succeed");
        let paths = store::StorePaths::new(store_root);
        let previous = lockfile::UserLock {
            schema_version: lockfile::USER_LOCK_SCHEMA_VERSION,
            active_instruction_packs: vec![lockfile::LockedInstructionPack {
                source_id: "local".to_owned(),
                pack_id: "old".to_owned(),
                target: PathBuf::from("/tmp/old"),
                commit: None,
                version: Some("1".to_owned()),
            }],
            ..lockfile::UserLock::default()
        };
        let next = lockfile::UserLock {
            schema_version: lockfile::USER_LOCK_SCHEMA_VERSION,
            ..lockfile::UserLock::default()
        };
        store::write_user_lock(&paths, &previous).expect("previous lock should be written");
        let writes = Cell::new(0);

        let error = write_sync_lock_with_rollback(&paths, &previous, &next, None, |paths, lock| {
            if writes.get() == 0 {
                writes.set(1);
                store::write_user_lock(paths, lock)?;
                return Err(DaloError::Io(std::io::Error::other("late lock failure")));
            }
            store::write_user_lock(paths, lock)
        })
        .expect_err("late lock failure should restore the previous lock");

        assert!(error.to_string().contains("late lock failure"));
        assert_eq!(
            store::read_user_lock(&paths).expect("previous lock should be restored"),
            previous
        );
    }
}
