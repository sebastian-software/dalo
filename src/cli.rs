//! Command-line parser and handlers.

use std::fs;
use std::io;
use std::path::PathBuf;

use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use clap_mangen::Man;

use crate::adopt;
use crate::agent;
use crate::approval;
use crate::audit;
use crate::autosync;
use crate::catalog;
use crate::config;
use crate::doctor;
use crate::error::{DaloError, DaloResult};
use crate::instructions;
use crate::inventory;
use crate::lockfile;
use crate::materialize;
use crate::resolver;
use crate::source;
use crate::status;
use crate::store;
use crate::target;
use crate::team_manifest;
use crate::update;

/// Parsed command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "dalo")]
#[command(
    version,
    about = "Git-backed skill management for AI agents.",
    long_about = "Git-backed skill management for AI agents.\n\nDalo keeps a local store of skill sources, resolves one approved skill set, and links that set into the folders your agents already read.",
    after_help = "Start here: dalo init -> dalo target link <agent> -> dalo source add <id> <git-url-or-path> -> dalo sync\nTry safely: use --store with a temporary directory and target link generic <path>.",
    after_long_help = "Mental model:\n  store   local database under ~/.dalo, or --store PATH\n  source  Git-backed skill collection, including the built-in local source\n  sync    refreshes clean tracking sources, resolves approved skills, and links them into targets\n\nQuickstart:\n  1. dalo init\n  2. dalo target link <codex|claude|openclaw|hermes|generic> [path]\n  3. dalo source add <id> <git-url-or-path>\n  4. dalo sync\n\nSafe sandbox:\n  export DALO_STORE=\"$(mktemp -d)/store\"\n  dalo init\n  dalo target link generic \"$(mktemp -d)/skills\""
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
    /// Inspect portable canonical agent packages and provider projections.
    Agent(AgentCommand),
    /// Author and maintain a team repository's `dalo.toml`.
    #[command(
        after_help = "Team commands act on a repository selected with --repo (default: the current directory). The global --store flag is accepted but has no effect here.\n\nExamples:\n  dalo team init company\n  dalo team catalog add marketing https://github.com/coreyhaines31/marketingskills.git --version <commit> --skill +copywriting\n  dalo team catalog skills marketing +copywriting +launch -seo-audit\n  dalo --dry-run team catalog update marketing --from main\n  dalo team show"
    )]
    Team(TeamCommand),
    /// Show managed, unmanaged, and conflicted skill state.
    Status(CheckArgs),
    /// Refresh clean sources, resolve approved skills, and link them into targets.
    #[command(
        long_about = "Refresh clean tracking sources, resolve the approved skill set, and materialize it into linked target folders.\n\nA skill source is a Git-backed collection of skills. Sync never overwrites unmanaged files; blocked or shadowed skills are reported instead."
    )]
    Sync(CheckArgs),
    /// Install, inspect, or remove scheduled synchronization.
    Autosync(AutosyncCommand),
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
        after_help = "Examples:\n  dalo audit public:review-helper\n  dalo audit ./my-skill --reviewer auto\n  dalo audit public:review-helper --reviewer codex --check\n  dalo audit public:review-helper --accept-risk 'reviewed upstream installer'"
    )]
    Audit(AuditCommand),
    /// Grant, list, and revoke scoped approval records.
    #[command(
        after_help = "Examples:\n  dalo approve list\n  dalo approve skill public:review-helper\n  dalo approve agent team:reviewer\n  dalo approve source team\n  dalo approve author public:maintainers\n  dalo approve org public:example-org\n  dalo approve revoke skill public:review-helper"
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

/// `autosync` command group.
#[derive(Debug, Args)]
pub struct AutosyncCommand {
    /// Autosync lifecycle command.
    #[command(subcommand)]
    pub command: AutosyncSubcommand,
}

/// Autosync lifecycle commands.
#[derive(Debug, Subcommand)]
pub enum AutosyncSubcommand {
    /// Install or update the current user's native scheduler job.
    Install(AutosyncInstallArgs),
    /// Inspect scheduler installation and the latest durable run result.
    Status,
    /// Disable and remove the current user's native scheduler job.
    Uninstall,
    /// Execute one non-interactive scheduled synchronization.
    #[command(hide = true)]
    Run,
}

/// Arguments for `autosync install`.
#[derive(Debug, Args)]
pub struct AutosyncInstallArgs {
    /// Schedule for the installed job.
    #[arg(long, value_enum)]
    pub schedule: Option<autosync::AutosyncSchedule>,
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
    /// Approve one source-qualified canonical agent package.
    Agent(AgentApprovalArgs),
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

    /// Semantic-review provider selection.
    #[command(flatten)]
    pub reviewer: ReviewerArgs,

    /// Ignore a compatible cached semantic review.
    #[arg(long = "refresh-audit", alias = "refresh")]
    pub refresh_audit: bool,

    /// Accept blocking findings for this exact content hash with a reason.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
}

/// One canonical agent approval value.
#[derive(Debug, Args)]
pub struct AgentApprovalArgs {
    /// Agent in `<source>:<name>` format, for example `team:reviewer`.
    #[arg(value_name = "VALUE")]
    pub value: String,
}

/// Arguments for `audit`.
#[derive(Debug, Args)]
pub struct AuditCommand {
    /// Existing skill path or source-qualified `<source>:<skill>` reference.
    pub target: String,

    /// Semantic-review provider selection.
    #[command(flatten)]
    pub reviewer: ReviewerArgs,

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

/// Semantic-review selection with a deprecated `--agent` compatibility alias.
#[derive(Debug, Args)]
pub struct ReviewerArgs {
    /// Run an isolated semantic review; this may send skill contents to its provider.
    #[arg(long, value_enum, default_value_t = AuditAgentArg::None)]
    pub reviewer: AuditAgentArg,

    /// Deprecated alias for `--reviewer`.
    #[arg(long = "agent", value_enum, hide = true, conflicts_with = "reviewer")]
    pub legacy_agent: Option<AuditAgentArg>,
}

impl ReviewerArgs {
    fn selected(&self) -> AuditAgentArg {
        self.legacy_agent.unwrap_or(self.reviewer)
    }
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
    /// Approval scope: skill, agent, source, author, or org.
    #[arg(value_enum)]
    pub scope: ApprovalScopeArg,
    /// Approval value in the format required by the selected scope.
    pub value: String,
}

/// Approval scopes accepted by `approve revoke`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ApprovalScopeArg {
    /// One source-qualified skill.
    Skill,
    /// One source-qualified canonical agent package.
    Agent,
    /// Every skill from one configured source.
    Source,
    /// One source-qualified author.
    Author,
    /// One source-qualified organization.
    Org,
}

impl ApprovalScopeArg {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Skill => "skill",
            Self::Agent => "agent",
            Self::Source => "source",
            Self::Author => "author",
            Self::Org => "org",
        }
    }
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

/// `agent` command group.
#[derive(Debug, Args)]
pub struct AgentCommand {
    /// Agent lifecycle command.
    #[command(subcommand)]
    pub command: AgentSubcommand,
}

/// Read-only agent subcommands available during the compiler foundation stage.
#[derive(Debug, Subcommand)]
pub enum AgentSubcommand {
    /// List discovered canonical agents and their deterministic approval state.
    List,
    /// Show a canonical agent and provider compilation previews without writing files.
    Show(AgentShowArgs),
}

/// Arguments for `agent show`.
#[derive(Debug, Args)]
pub struct AgentShowArgs {
    /// Agent in `<source>:<name>` format, for example `local:reviewer`.
    pub agent: String,

    /// Restrict previews to one provider.
    #[arg(long, value_enum)]
    pub provider: Option<AgentProviderArg>,
}

/// Provider selection exposed by canonical-agent previews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AgentProviderArg {
    /// Anthropic Claude Code agent format.
    Claude,
    /// OpenAI Codex agent format.
    Codex,
}

impl From<AgentProviderArg> for agent::AgentProvider {
    fn from(value: AgentProviderArg) -> Self {
        match value {
            AgentProviderArg::Claude => Self::Claude,
            AgentProviderArg::Codex => Self::Codex,
        }
    }
}

/// `source` subcommands.
#[derive(Debug, Subcommand)]
pub enum SourceSubcommand {
    /// Add a team source from a Git URL or local path.
    Add(SourceAddArgs),
    /// Add a catalog source (a multi-skill repository) from a Git URL or local path.
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
    /// Inspect or explicitly advance a pinned catalog source.
    #[command(
        after_help = "Examples:\n  dalo source refresh public\n  dalo source refresh public --check\n  dalo --dry-run --json source refresh public --advance\n  dalo source refresh public --advance"
    )]
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

    /// Git URL or local path of the team source.
    pub location: String,
}

/// Arguments for `source add-catalog`.
#[derive(Debug, Args)]
pub struct SourceAddCatalogArgs {
    /// Source ID.
    pub id: String,

    /// Git URL or local path of the catalog source.
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

    /// Advance the catalog pin after previewing and validating the candidate.
    #[arg(long, conflicts_with = "check")]
    pub advance: bool,
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

/// `team` manifest management command group.
#[derive(Debug, Args)]
pub struct TeamCommand {
    /// Team repository directory. Defaults to the current directory.
    #[arg(long, default_value = ".", value_name = "PATH")]
    pub repo: PathBuf,

    /// Team management subcommand.
    #[command(subcommand)]
    pub command: TeamSubcommand,
}

/// `team` subcommands.
#[derive(Debug, Subcommand)]
pub enum TeamSubcommand {
    /// Create a `dalo.toml` for this team repository.
    Init(TeamInitArgs),
    /// Show the parsed team manifest.
    Show,
    /// Add, update, or remove external catalogs.
    Catalog(TeamCatalogCommand),
}

/// Arguments for `team init`.
#[derive(Debug, Args)]
pub struct TeamInitArgs {
    /// Stable team source ID used when team members add this repository.
    pub id: String,

    /// Optional human-readable team source name.
    #[arg(long)]
    pub name: Option<String>,
}

/// `team catalog` command group.
#[derive(Debug, Args)]
pub struct TeamCatalogCommand {
    /// Catalog management subcommand.
    #[command(subcommand)]
    pub command: TeamCatalogSubcommand,
}

/// `team catalog` subcommands.
#[derive(Debug, Subcommand)]
pub enum TeamCatalogSubcommand {
    /// Add a pinned external catalog.
    Add(TeamCatalogAddArgs),
    /// Replace a catalog's include/exclude filters. No filters means all.
    Skills(TeamCatalogSkillsArgs),
    /// Change a catalog's pinned version.
    Version(TeamCatalogVersionArgs),
    /// Resolve an upstream ref and review its next exact commit pin.
    Update(TeamCatalogUpdateArgs),
    /// Remove a catalog declaration.
    Remove(TeamCatalogIdArgs),
}

/// Arguments for `team catalog add`.
#[derive(Debug, Args)]
pub struct TeamCatalogAddArgs {
    /// Catalog ID local to this team manifest.
    pub id: String,

    /// Git URL or path of the external skill set.
    pub url: String,

    /// Git commit, tag, or ref to pin.
    #[arg(long)]
    pub version: String,

    /// Skill include/exclude filter, repeatable. Omit to select all.
    #[arg(long = "skill", value_name = "FILTER", allow_hyphen_values = true)]
    pub skills: Vec<String>,

    /// Optional global resolver priority.
    #[arg(long)]
    pub priority: Option<i32>,
}

/// Arguments for `team catalog skills`.
#[derive(Debug, Args)]
pub struct TeamCatalogSkillsArgs {
    /// Catalog ID.
    pub id: String,

    /// New filters. Empty means all; `+` includes and `-` excludes.
    #[arg(allow_hyphen_values = true)]
    pub skills: Vec<String>,
}

/// Arguments for `team catalog version`.
#[derive(Debug, Args)]
pub struct TeamCatalogVersionArgs {
    /// Catalog ID.
    pub id: String,

    /// New Git commit, tag, or ref.
    pub version: String,
}

/// Arguments for `team catalog update`.
#[derive(Debug, Args)]
pub struct TeamCatalogUpdateArgs {
    /// Catalog ID.
    pub id: String,

    /// Upstream branch, tag, or ref to resolve and review.
    #[arg(long = "from", value_name = "REF")]
    pub from_ref: String,

    /// Accept blocking security-audit findings for this exact reviewed candidate.
    #[arg(long, value_name = "REASON")]
    pub accept_risk: Option<String>,
}

/// Catalog ID argument.
#[derive(Debug, Args)]
pub struct TeamCatalogIdArgs {
    /// Catalog ID.
    pub id: String,
}

/// Arguments for `adopt`.
#[derive(Debug, Args)]
pub struct AdoptCommand {
    /// Skill slot name or path to adopt.
    pub skill: String,

    /// Replace the original unmanaged folder with an owned symlink after copying.
    #[arg(long)]
    pub replace: bool,

    /// Semantic-review provider selection.
    #[command(flatten)]
    pub reviewer: ReviewerArgs,

    /// Ignore a compatible cached semantic review.
    #[arg(long = "refresh-audit", alias = "refresh")]
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

    /// Semantic-review provider selection.
    #[command(flatten)]
    pub reviewer: ReviewerArgs,

    /// Ignore a compatible cached semantic review.
    #[arg(long = "refresh-audit", alias = "refresh")]
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

    let options = if matches!(command, Command::Team(_)) {
        GlobalOptions {
            store: PathBuf::new(),
            json,
            yes,
            dry_run,
        }
    } else {
        GlobalOptions::resolve(store.as_deref(), json, yes, dry_run)?
    };
    if command_ignores_dry_run(&command) {
        warn_noop_dry_run(options.dry_run, options.json);
    }

    let result = match command {
        Command::Init => run_init(&options),
        Command::Target(command) => run_target(&options, command),
        Command::Source(command) => run_source(&options, command),
        Command::Agent(command) => run_agent(&options, command),
        Command::Team(command) => run_team(&options, command),
        Command::Status(args) => run_status(&options, args),
        Command::Sync(args) => run_sync(&options, args),
        Command::Autosync(command) => run_autosync(&options, command),
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
            command: SourceSubcommand::List | SourceSubcommand::Inspect(_)
        }) | Command::Agent(AgentCommand {
            command: AgentSubcommand::List | AgentSubcommand::Show(_)
        }) | Command::Status(_)
            | Command::Doctor(_)
            | Command::Approve(ApproveCommand {
                command: ApproveSubcommand::List
            })
            | Command::Instructions(InstructionsCommand {
                command: InstructionsSubcommand::List
            })
            | Command::Autosync(AutosyncCommand {
                command: AutosyncSubcommand::Status
            })
            | Command::Team(TeamCommand {
                command: TeamSubcommand::Show,
                ..
            })
            | Command::Resolve(ResolveCommand {
                command: ResolveSubcommand::List
            })
    ) || matches!(
        command,
        Command::Source(SourceCommand {
            command: SourceSubcommand::Refresh(SourceRefreshArgs { advance: false, .. })
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

fn run_agent(options: &GlobalOptions, command: AgentCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let config = store::read_config(&paths)?;
    let approvals = store::read_approvals(&paths)?;
    let mut inventories = Vec::new();
    let mut warnings = Vec::new();
    let mut source_errors = Vec::new();
    for source in config.sources.iter().filter(|source| source.enabled) {
        match inventory::scan_source(&source.id, &source.path) {
            Ok(inventory) => {
                warnings.extend(inventory.agent_warnings.iter().cloned());
                inventories.push(inventory);
            }
            Err(error) => source_errors.push(format!("{}: {error}", source.id)),
        }
    }
    warnings.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.as_str().cmp(right.code.as_str()))
    });
    source_errors.sort();

    match command.command {
        AgentSubcommand::List => {
            let report = agent::AgentListReport {
                resolution: agent::resolve_agents(
                    &config.sources,
                    &inventories,
                    &approvals.approvals,
                ),
                inventory_warnings: warnings,
                source_errors,
            };
            if options.json {
                print_json(&report)?;
            } else {
                print_agent_list_report(&report);
            }
        }
        AgentSubcommand::Show(args) => {
            if !source_errors.is_empty() {
                return Err(DaloError::StateError {
                    reason: format!(
                        "cannot inspect canonical agents because source inventory failed: {}",
                        source_errors.join(", ")
                    ),
                });
            }
            let record = agent::find_agent(&config.sources, &inventories, &args.agent)?;
            let providers = args.provider.map_or_else(
                || vec![agent::AgentProvider::Claude, agent::AgentProvider::Codex],
                |provider| vec![provider.into()],
            );
            let report = agent::AgentShowReport {
                compilations: providers
                    .into_iter()
                    .map(|provider| agent::compile_record(&record, provider))
                    .collect(),
                agent: record,
            };
            if options.json {
                print_json(&report)?;
            } else {
                print_agent_show_report(&report);
            }
        }
    }
    Ok(())
}

fn print_agent_list_report(report: &agent::AgentListReport) {
    if report.resolution.active_agents.is_empty()
        && report.resolution.pending_approval_agents.is_empty()
        && report.resolution.shadowed_agents.is_empty()
    {
        println!("no canonical agents discovered");
    }
    for active in &report.resolution.active_agents {
        println!(
            "active {} (priority {})",
            active.agent.source_ref, active.source_priority
        );
    }
    for pending in &report.resolution.pending_approval_agents {
        println!(
            "pending approval {} (run: dalo approve agent {})",
            pending.agent.source_ref, pending.agent.source_ref
        );
    }
    for shadowed in &report.resolution.shadowed_agents {
        println!(
            "shadowed {} by {}",
            shadowed.agent.agent.source_ref, shadowed.shadowed_by
        );
    }
    for warning in &report.inventory_warnings {
        println!(
            "warning {}: {} ({})",
            warning.code,
            warning.path.display(),
            warning.message
        );
    }
    for error in &report.source_errors {
        println!("source error {error}");
    }
}

fn print_agent_show_report(report: &agent::AgentShowReport) {
    println!("{}", report.agent.source_ref);
    println!("description: {}", report.agent.description);
    println!("package hash: {}", report.agent.content_hash);
    for compilation in &report.compilations {
        let visible_findings = compilation
            .findings
            .iter()
            .filter(|finding| !finding.is_dalo_identity_metadata())
            .collect::<Vec<_>>();
        let status = if compilation.not_targeted {
            "not targeted".to_owned()
        } else {
            visible_findings
                .iter()
                .map(|finding| finding.result)
                .max()
                .unwrap_or(agent::CompatibilityResult::Exact)
                .to_string()
        };
        println!("{}: {status}", compilation.provider.id());
        for finding in visible_findings {
            println!(
                "  {}: {} — {}",
                finding.field, finding.result, finding.message
            );
        }
    }
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
                print_json(&instructions::InstructionPackListReport {
                    active_instruction_packs: lock.active_instruction_packs,
                })?;
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

    if args.check
        && let Some(reason) = status_review_reason(&report)
    {
        return Err(DaloError::CheckFailed { reason });
    }
    Ok(())
}

fn status_review_reason(report: &status::StatusReport) -> Option<String> {
    let mut reasons = Vec::new();

    if !report.inventory_warnings.is_empty() {
        let count = report.inventory_warnings.len();
        reasons.push(format!(
            "{count} inventory warning{} ({})",
            if count == 1 { "" } else { "s" },
            report
                .inventory_warnings
                .iter()
                .map(|warning| warning.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !report.agent_inventory_warnings.is_empty() {
        let count = report.agent_inventory_warnings.len();
        reasons.push(format!(
            "{count} agent inventory warning{} ({})",
            if count == 1 { "" } else { "s" },
            report
                .agent_inventory_warnings
                .iter()
                .map(|warning| warning.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let source_errors = report
        .sources
        .iter()
        .filter(|source| source.error.is_some())
        .collect::<Vec<_>>();
    if !source_errors.is_empty() {
        reasons.push(format!(
            "{} source error{} ({})",
            source_errors.len(),
            if source_errors.len() == 1 { "" } else { "s" },
            source_errors
                .iter()
                .map(|source| source.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
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

    if !report.blocking_audits.is_empty() {
        reasons.push(format!(
            "{} blocking security audit{} ({})",
            report.blocking_audits.len(),
            if report.blocking_audits.len() == 1 {
                ""
            } else {
                "s"
            },
            report.blocking_audits.join(", ")
        ));
    }

    if !report.audit_failures.is_empty() {
        reasons.push(format!(
            "{} failed security audit{} ({})",
            report.audit_failures.len(),
            if report.audit_failures.len() == 1 {
                ""
            } else {
                "s"
            },
            report
                .audit_failures
                .iter()
                .map(|failure| failure.source_ref.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let blocked_operations = report
        .materialization
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
                && (report.audit_failures.is_empty()
                    || diagnostic.code != resolver::ResolutionDiagnosticCode::AuditFailed)
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

    if !report.lock.drift.is_empty() {
        reasons.push(format!(
            "{} lock drift item{} ({})",
            report.lock.drift.len(),
            if report.lock.drift.len() == 1 {
                ""
            } else {
                "s"
            },
            report
                .lock
                .drift
                .iter()
                .map(|drift| drift.subject.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if report.targets.is_empty() && !report.resolution.active_skills.is_empty() {
        let count = report.resolution.active_skills.len();
        reasons.push(format!(
            "{count} active skill{} but no linked targets",
            if count == 1 { "" } else { "s" }
        ));
    }

    let unmanaged_skills = report
        .unmanaged_skills
        .iter()
        .filter(|skill| !skill.protected)
        .collect::<Vec<_>>();
    if !unmanaged_skills.is_empty() {
        reasons.push(format!(
            "{} unmanaged skill{} ({})",
            unmanaged_skills.len(),
            if unmanaged_skills.len() == 1 { "" } else { "s" },
            unmanaged_skills
                .iter()
                .map(|skill| skill.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if !report.instruction_block_drifts.is_empty() {
        reasons.push(format!(
            "{} instruction block drift{} ({})",
            report.instruction_block_drifts.len(),
            if report.instruction_block_drifts.len() == 1 {
                ""
            } else {
                "s"
            },
            report
                .instruction_block_drifts
                .iter()
                .map(|drift| format!("{}:{}", drift.source_id, drift.pack_id))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    if report.autosync.scheduler_error.is_some() {
        reasons.push("autosync scheduler inspection failed".to_owned());
    }
    if report.autosync.configured != report.autosync.installed {
        reasons.push("autosync configuration and installation state differ".to_owned());
    }
    if report.autosync.installed && !report.autosync.enabled {
        reasons.push("autosync scheduler is disabled".to_owned());
    }
    if report.autosync.installed
        && report
            .autosync
            .last_run
            .as_ref()
            .is_some_and(|run| run.outcome == autosync::AutosyncRunOutcome::Blocked)
    {
        reasons.push("latest autosync run was blocked".to_owned());
    }
    if report.autosync.installed
        && report.autosync.last_run.as_ref().is_some_and(|run| {
            autosync::running_run_is_stale(run, report.autosync.schedule, autosync::now_unix())
        })
    {
        reasons.push("latest autosync run started but never finished".to_owned());
    }

    (!reasons.is_empty()).then(|| reasons.join(", "))
}

fn run_sync(options: &GlobalOptions, args: CheckArgs) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    let _lock = if options.dry_run {
        None
    } else {
        Some(store::StoreLock::acquire(&paths)?)
    };
    run_sync_locked(options, args)
}

fn run_sync_locked(options: &GlobalOptions, args: CheckArgs) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    let _catalog_lock = if options.dry_run {
        store::CatalogLock::acquire_shared(&paths)?
    } else {
        store::CatalogLock::acquire_exclusive(&paths)?
    };
    if options.dry_run {
        catalog::ensure_no_pending_catalog_advance(&paths)?;
    } else {
        catalog::recover_pending_catalog_advance(&paths)?;
    }
    // Read the existing lock before mutating sources or targets. A malformed or
    // newer lock is recovery data, not an empty baseline we are allowed to
    // overwrite after a successful materialization pass.
    let previous = if options.dry_run {
        None
    } else {
        Some(store::read_user_lock(&paths)?)
    };
    let config = store::read_config(&paths)?;
    let unrefreshed_tracking_sources = if options.dry_run {
        tracking_team_source_ids(&config)
    } else {
        Vec::new()
    };
    let refresh_failures = if options.dry_run {
        Vec::new()
    } else {
        source::refresh_tracking_team_sources_from_config(&paths, &config)?
    };
    let (manifest_report, manifest_rollback) = if options.dry_run {
        (None, None)
    } else {
        let (report, rollback) = team_manifest::reconcile_team_manifests(&paths)?;
        (Some(report), Some(rollback))
    };
    let config = if options.dry_run {
        team_manifest::preview_team_manifests(&paths)?
    } else {
        store::read_config(&paths)?
    };
    let sync_result = (|| -> DaloResult<materialize::SyncReport> {
        let approvals = store::read_approvals(&paths)?;
        let mut live = resolver::resolve_from_config(&config, approvals.approvals);
        let audits = audit::audit_active_skills(&paths, &live.resolution, !options.dry_run);
        ensure_no_blocking_audits(&audits.blocking)?;
        resolver::degrade_audit_failures(&mut live.resolution, &audits.failures);
        let degraded_sources = collect_degraded_sources(&live, refresh_failures, &audits.failures);
        let (mut report, rollback) = materialize::materialize_with_degraded_sources_rollback(
            &paths,
            &live.resolution,
            options.dry_run,
            &degraded_sources,
        )?;
        report.unselected_catalogs = unselected_catalogs(&live);
        report.unrefreshed_tracking_sources = unrefreshed_tracking_sources;
        if !options.dry_run {
            let previous = previous
                .as_ref()
                .expect("non-dry-run sync reads the user lock before materializing");
            let mut lock =
                lockfile::build_user_lock(&config.sources, &live.resolution, Some(&report));
            // Instruction packs are owned by the `instructions` command; preserve them
            // across a sync instead of dropping them.
            lock.active_instruction_packs = previous.active_instruction_packs.clone();
            write_sync_lock_with_rollback(
                &paths,
                previous,
                &lock,
                rollback,
                store::write_user_lock,
            )?;
        }
        Ok(report)
    })();
    let report = match sync_result {
        Ok(report) => report,
        Err(error) => {
            if let Some(rollback) = manifest_rollback
                && let Err(rollback_error) = rollback.rollback(&paths)
            {
                return Err(DaloError::Io(std::io::Error::other(format!(
                    "{error}; additionally failed to roll back team manifest changes: {rollback_error}"
                ))));
            }
            return Err(error);
        }
    };
    if let Some(manifest_report) = &manifest_report {
        team_manifest::cleanup_removed_checkouts(&paths, &manifest_report.removed);
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

fn tracking_team_source_ids(config: &config::UserConfig) -> Vec<String> {
    config
        .sources
        .iter()
        .filter(|source| {
            source.enabled
                && source.kind == source::SourceKind::Team
                && source.update_policy.as_deref() == Some("track")
        })
        .map(|source| source.id.clone())
        .collect()
}

fn run_autosync(options: &GlobalOptions, command: AutosyncCommand) -> DaloResult<()> {
    let paths = store::StorePaths::new(options.store.clone());
    ensure_initialized(&paths)?;
    match command.command {
        AutosyncSubcommand::Install(args) => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = autosync::install(&paths, args.schedule, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_autosync_mutation_report(&report);
            }
            Ok(())
        }
        AutosyncSubcommand::Status => {
            let report = autosync::status(&paths)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_autosync_status_report(&report);
            }
            Ok(())
        }
        AutosyncSubcommand::Uninstall => {
            let _lock = if options.dry_run {
                None
            } else {
                Some(store::StoreLock::acquire(&paths)?)
            };
            let report = autosync::uninstall(&paths, options.dry_run)?;
            if options.json {
                print_json(&report)?;
            } else {
                status::print_autosync_mutation_report(&report);
            }
            Ok(())
        }
        AutosyncSubcommand::Run => run_scheduled_sync(options, &paths),
    }
}

fn run_scheduled_sync(options: &GlobalOptions, paths: &store::StorePaths) -> DaloResult<()> {
    if options.dry_run {
        return Err(DaloError::CheckFailed {
            reason: "the internal scheduled runner does not support --dry-run".to_owned(),
        });
    }
    // Bound the append-only scheduler logs before this run appends to them.
    autosync::trim_scheduler_logs(paths);
    let attempted = autosync::begin_run(paths)?;
    let Some(_lock) = store::StoreLock::try_acquire(paths)? else {
        let holder = store::store_lock_holder(paths).map_or_else(
            || "store lock is held".to_owned(),
            |holder| format!("store lock held by {holder}"),
        );
        autosync::finish_run(
            paths,
            attempted,
            autosync::AutosyncRunOutcome::Skipped,
            Some(holder.clone()),
        )?;
        println!("autosync skipped: {holder}");
        return Ok(());
    };

    let result = run_sync_locked(options, CheckArgs { check: true })
        .and_then(|()| scheduled_sync_postflight(paths));
    match result {
        Ok(()) => autosync::finish_run(
            paths,
            attempted,
            autosync::AutosyncRunOutcome::Succeeded,
            None,
        ),
        Err(error) => {
            let status_result = autosync::finish_run(
                paths,
                attempted,
                autosync::AutosyncRunOutcome::Blocked,
                Some(error.to_string()),
            );
            if let Err(status_error) = status_result {
                return Err(DaloError::Io(std::io::Error::other(format!(
                    "{error}; additionally failed to persist autosync status: {status_error}"
                ))));
            }
            Err(error)
        }
    }
}

fn scheduled_sync_postflight(paths: &store::StorePaths) -> DaloResult<()> {
    let config = store::read_config(paths)?;
    let lock = store::read_user_lock(paths)?;
    let instruction_drifts = instructions::instruction_block_drifts(
        paths,
        &config.sources,
        &lock.active_instruction_packs,
    );
    if !instruction_drifts.is_empty() {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "{} managed instruction block{} drifted; inspect with `dalo status`",
                instruction_drifts.len(),
                if instruction_drifts.len() == 1 {
                    ""
                } else {
                    "s"
                }
            ),
        });
    }

    let mut removed = Vec::new();
    for source in config
        .sources
        .iter()
        .filter(|source| source.enabled && source.kind == source::SourceKind::Catalog)
    {
        let drift = catalog::check_catalog_drift(paths, &source.id)?;
        status::print_catalog_drift_report(&drift);
        removed.extend(
            drift
                .outcomes
                .into_iter()
                .filter(|outcome| outcome.code.blocks_sync())
                .map(|outcome| format!("{}:{}", source.id, outcome.skill)),
        );
    }
    if !removed.is_empty() {
        return Err(DaloError::CheckFailed {
            reason: format!(
                "selected catalog skills were removed upstream ({}); update the selection or keep the existing pin",
                removed.join(", ")
            ),
        });
    }
    Ok(())
}

fn ensure_no_blocking_audits(blocking_audits: &[String]) -> DaloResult<()> {
    if blocking_audits.is_empty() {
        return Ok(());
    }
    Err(DaloError::AuditBlocked {
        reason: format!(
            "security audit blocked {} skill{} ({}); inspect with `dalo audit <source:skill>` or record an explicit `--accept-risk` reason",
            blocking_audits.len(),
            if blocking_audits.len() == 1 { "" } else { "s" },
            blocking_audits.join(", ")
        ),
    })
}

fn collect_degraded_sources(
    live: &resolver::LiveResolution,
    refresh_failures: Vec<source::TrackingSourceRefreshFailure>,
    audit_failures: &[audit::ActiveAuditFailure],
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
    for failure in audit_failures {
        let reason = format!(
            "security audit failed for {}: {}",
            failure.source_ref, failure.reason
        );
        if let Some(existing) = degraded_sources
            .iter_mut()
            .find(|source| source.id == failure.source_id)
        {
            existing.reason = format!("{}; {reason}", existing.reason);
        } else if let Some(scan) = live
            .scans
            .iter()
            .find(|scan| scan.source.id == failure.source_id)
        {
            degraded_sources.push(materialize::DegradedSource {
                id: failure.source_id.clone(),
                path: scan.source.path.clone(),
                reason,
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

fn unselected_catalogs(live: &resolver::LiveResolution) -> Vec<materialize::UnselectedCatalog> {
    let mut catalogs = live
        .scans
        .iter()
        .filter(|scan| {
            scan.source.kind == source::SourceKind::Catalog && scan.source.selection.is_empty()
        })
        .filter_map(|scan| {
            let available_skills = scan.inventory.as_ref()?.skills.len();
            (available_skills > 0).then(|| materialize::UnselectedCatalog {
                source_id: scan.source.id.clone(),
                available_skills,
            })
        })
        .collect::<Vec<_>>();
    catalogs.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    catalogs
}

fn run_team(options: &GlobalOptions, command: TeamCommand) -> DaloResult<()> {
    let repo = command.repo;
    match command.command {
        TeamSubcommand::Init(args) => {
            let report = team_manifest::init_team_manifest(
                &repo,
                &args.id,
                args.name.as_deref(),
                options.dry_run,
            )?;
            print_team_manifest_mutation(options, &report)
        }
        TeamSubcommand::Show => {
            let report = team_manifest::show_team_manifest(&repo)?;
            if options.json {
                print_json(&report)
            } else {
                status::print_team_manifest_view(&report);
                Ok(())
            }
        }
        TeamSubcommand::Catalog(command) => {
            let report = match command.command {
                TeamCatalogSubcommand::Update(args) => {
                    let report = team_manifest::update_team_catalog_pin(
                        &repo,
                        &args.id,
                        &args.from_ref,
                        options.dry_run,
                        args.accept_risk.as_deref(),
                    )?;
                    if options.json {
                        print_json(&report)?;
                    } else {
                        status::print_team_catalog_update(&report);
                    }
                    if !report.blocking_reasons.is_empty() {
                        return Err(DaloError::StateError {
                            reason: format!(
                                "team catalog pin was not updated: {}",
                                report.blocking_reasons.join("; ")
                            ),
                        });
                    }
                    return Ok(());
                }
                TeamCatalogSubcommand::Add(args) => team_manifest::add_team_catalog(
                    &repo,
                    &args.id,
                    &args.url,
                    &args.version,
                    &args.skills,
                    args.priority,
                    options.dry_run,
                )?,
                TeamCatalogSubcommand::Skills(args) => team_manifest::set_team_catalog_skills(
                    &repo,
                    &args.id,
                    &args.skills,
                    options.dry_run,
                )?,
                TeamCatalogSubcommand::Version(args) => team_manifest::set_team_catalog_version(
                    &repo,
                    &args.id,
                    &args.version,
                    options.dry_run,
                )?,
                TeamCatalogSubcommand::Remove(args) => {
                    team_manifest::remove_team_catalog(&repo, &args.id, options.dry_run)?
                }
            };
            print_team_manifest_mutation(options, &report)
        }
    }
}

fn print_team_manifest_mutation(
    options: &GlobalOptions,
    report: &team_manifest::TeamManifestMutationReport,
) -> DaloResult<()> {
    if options.json {
        print_json(report)
    } else {
        status::print_team_manifest_mutation(report);
        Ok(())
    }
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
            let outcome =
                catalog::add_catalog_source(&paths, &args.id, &location, options.dry_run)?;
            if options.json {
                print_json(&outcome.source)?;
            } else {
                status::print_catalog_add_report(
                    &outcome.source,
                    outcome.available_skills,
                    options.dry_run,
                );
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
            let _lock = if args.advance && !options.dry_run {
                Some(store::StoreLock::acquire(&paths)?)
            } else {
                None
            };
            let _catalog_lock = if args.advance && !options.dry_run {
                Some(store::CatalogLock::acquire_exclusive(&paths)?)
            } else {
                None
            };
            if args.advance {
                let report = catalog::advance_catalog(&paths, &args.id, options.dry_run)?;
                if options.json {
                    print_json(&report)?;
                } else {
                    status::print_catalog_advance_report(&report);
                }
                if !report.blocking_reasons.is_empty() {
                    return Err(DaloError::StateError {
                        reason: format!(
                            "catalog pin was not advanced: {}",
                            report.blocking_reasons.join("; ")
                        ),
                    });
                }
                return Ok(());
            }
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
    let mut live = resolver::resolve_from_config(&plan.config, plan.approvals.approvals.clone());
    let audits = audit::audit_active_skills(paths, &live.resolution, !options.dry_run);
    ensure_no_blocking_audits(&audits.blocking)?;
    resolver::degrade_audit_failures(&mut live.resolution, &audits.failures);
    let degraded_sources = collect_degraded_sources(&live, Vec::new(), &audits.failures);
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
    if !args.keep_checkout {
        let checkouts = std::iter::once(&plan.report.checkout_path)
            .chain(plan.report.cascaded_checkout_paths.iter())
            .cloned()
            .collect::<Vec<_>>();
        for checkout in checkouts {
            if let Err(error) = cleanup_removed_source_checkout(&checkout) {
                plan.report.cleanup_warnings.push(format!(
                    "checkout cleanup incomplete for `{}`: {error}",
                    checkout.display()
                ));
            }
        }
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
        .filter(|skill| {
            skill.source_id == report.source_id
                || report.cascaded_sources.contains(&skill.source_id)
        })
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
    let Some(source_id) = source_id.to_str() else {
        return Ok(());
    };
    let Some(sources_dir) = source_dir.parent() else {
        return Ok(());
    };
    let staging_root = sources_dir.join(".audit-staging");
    let Ok(entries) = std::fs::read_dir(&staging_root) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if !source::staging_entry_belongs_to_source(&entry.file_name(), source_id) {
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
        command.reviewer.selected(),
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
                args.reviewer.selected(),
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
    let agent = prepare_agent_review(command.reviewer.selected())?;
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
            let agent = prepare_agent_review(args.reviewer.selected())?;
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
                return Err(DaloError::AuditBlocked {
                    reason: format!(
                        "refusing to approve `{}` until its findings are reviewed or accepted with `--accept-risk <reason>`",
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
        ApproveSubcommand::Agent(args) => print_approval_result(
            options,
            approval::grant(&paths, "agent", &args.value, options.dry_run)?,
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
            approval::revoke(&paths, args.scope.as_str(), &args.value, options.dry_run)?,
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
    adopt::validate_adoptable_slot_name(&unmanaged, selector)?;
    let agent = prepare_agent_review(agent)?;
    let audit_report = audit::audit_skill(
        paths,
        &format!("local:{}", unmanaged.slot_name),
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
