//! Native CLI integration for the SDK's optional usage reporting.

use std::env;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use app_tracking::client::{Client, Error, Interaction, PermissionOutcome, Status};
use app_tracking::project::{Origin, RepositoryContext};
use app_tracking::telemetry::{CONSENT_REVISION, Policy};
use clap::{Args, Subcommand};

use crate::cli::{AutosyncSubcommand, Cli, Command, HookSubcommand};
use crate::update::env_truthy;
use crate::{DaloError, DaloResult};

const SERVICE: &str = "https://sebastian-app-tracking.b-cdn.net";
const CI_ROOT_ENV: &str = "DALO_TELEMETRY_CI_STATE";
const DISCLOSURE: &str = "Help Sebastian Software GmbH understand which Dalo commands and platforms to maintain.

Optional usage reporting counts commands locally and normally sends daily HTTPS batches while Dalo is used. Reports include the UTC day, tool version, approved command category and count, random tool/profile installation and batch IDs, consent time, and local or CI context. System context can include operating system/version, architecture, CPU family, and Linux distribution/kernel version.

When reliable project context exists, the SDK sends a SHA-256 hash of the normalized repository origin. This lets equivalent clones share a project identity. A guessed URL can be matched to its hash. The reports are pseudonymous. Command arguments, raw URLs, paths, skill names or content, usernames, and machine names are excluded.

Active reports expire after at most 90 days. Bunny hosts the HTTPS service and PlanetScale stores reports. See the privacy notice for processor and transfer details, backup retention, your rights, and how to contact the controller.

Use dalo telemetry status and preview to inspect local data. Disable discards queued usage. Delete requests authenticated removal of retained reports and remains pending until acknowledged. Update checks keep their own settings. CI requires separate explicit authorization.

Reporting details: https://app-tracking.sebastian-software.com/
Privacy notice: https://sebastian-software.com/privacy#tool-usage-reports";

/// Controls for optional usage reporting.
#[derive(Debug, Args)]
pub struct TelemetryCommand {
    /// Reporting operation.
    #[command(subcommand)]
    pub command: TelemetryAction,
}

/// Operations on the SDK's private reporting state.
#[derive(Clone, Copy, Debug, Subcommand)]
pub enum TelemetryAction {
    /// Inspect the saved decision, queue and pending deletion.
    Status,
    /// Show queued usage locally, without credentials.
    Preview,
    /// Review the disclosure and explicitly enable reporting.
    Enable,
    /// Apply explicit operator authorization in an isolated CI job.
    EnableCi,
    /// Withdraw permission and discard unsent usage.
    Disable,
    /// Stop reporting and request authenticated removal of retained data.
    Delete,
}

fn profile_root() -> Option<PathBuf> {
    env::var_os("XDG_STATE_HOME")
        .filter(|path| Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| Path::new(path).is_absolute())
                .map(|home| PathBuf::from(home).join(".local/state"))
        })
        .map(|root| root.join("dalo/app-tracking"))
}

fn ci_root(personal: Option<&Path>) -> Option<PathBuf> {
    let root = PathBuf::from(env::var_os(CI_ROOT_ENV)?);
    let metadata = fs::symlink_metadata(&root).ok()?;
    if !root.is_absolute()
        || !metadata.is_dir()
        || metadata.permissions().mode() & 0o077 != 0
        || metadata.uid() != rustix::process::getuid().as_raw()
    {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    if let Some(personal) = personal {
        let personal = crate::store::comparable_path(personal);
        if root.starts_with(&personal) || personal.starts_with(&root) {
            return None;
        }
    }
    Some(root)
}

fn client() -> DaloResult<(Client, Policy)> {
    let ci = env_truthy("CI") || env_truthy("CONTINUOUS_INTEGRATION");
    let personal = profile_root();
    let root = if ci {
        ci_root(personal.as_deref()).ok_or_else(|| invalid(
            "CI reporting requires DALO_TELEMETRY_CI_STATE to name an existing private directory for this job",
        ))?
    } else {
        personal
            .ok_or_else(|| invalid("usage reporting requires an absolute XDG_STATE_HOME or HOME"))?
    };
    let policy = Policy {
        offline: env_truthy("DALO_OFFLINE"),
        do_not_track: env_truthy("DO_NOT_TRACK"),
        telemetry: env::var_os("APP_TRACKING_TELEMETRY")
            .map(|value| value.to_string_lossy().into_owned()),
        ci,
        ci_revision: env::var("APP_TRACKING_CI").ok(),
        isolated_ci_state: ci,
        ..Policy::default()
    };
    let client = Client::registered(SERVICE, root, "dalo", env!("CARGO_PKG_VERSION"))
        .map_err(sdk_error)?
        .with_policy(policy.clone())
        .with_interaction(Interaction::Never);
    Ok((client, policy))
}

fn reporting_blocked(policy: &Policy) -> bool {
    policy.do_not_track
        || policy.telemetry.as_deref().is_some_and(|mode| mode != "on")
        || (policy.ci && policy.ci_revision.as_deref() != Some(CONSENT_REVISION))
}

fn category(command: Option<&Command>) -> Option<&'static str> {
    Some(match command {
        Some(Command::Init) => "init",
        Some(Command::Install) => "install",
        Some(Command::Migrate(_)) => "migrate",
        Some(Command::Project(_)) => "project",
        Some(Command::Promote(_)) => "promote",
        Some(Command::Target(_)) => "target",
        Some(Command::Source(_)) => "source",
        Some(Command::Agent(_)) => "agent",
        Some(Command::Plugin(_)) => "plugin",
        Some(Command::Tool(_)) => "tool",
        Some(Command::Hook(command)) if matches!(command.command, HookSubcommand::Dispatch(_)) => {
            return None;
        }
        Some(Command::Hook(_)) => "hook",
        Some(Command::Binary(_)) => "binary",
        Some(Command::Plan(_)) => "plan",
        Some(Command::Team(_)) => "team",
        Some(Command::Status(_)) => "status",
        Some(Command::Next) => "next",
        Some(Command::Sync(_)) => "sync",
        Some(Command::Autosync(command)) if matches!(command.command, AutosyncSubcommand::Run) => {
            return None;
        }
        Some(Command::Autosync(_)) => "autosync",
        Some(Command::Adopt(_)) => "adopt",
        Some(Command::Resolve(_)) => "resolve",
        Some(Command::Doctor(_)) => "doctor",
        Some(Command::Audit(_)) => "audit",
        Some(Command::Approve(_)) => "approve",
        Some(Command::Instructions(_)) => "instructions",
        Some(
            Command::Telemetry(_)
            | Command::Assistant(_)
            | Command::Completions(_)
            | Command::Manpage,
        ) => {
            return None;
        }
        None => "other",
    })
}

/// Count one native CLI invocation if already authorized. This never prompts,
/// blocks on HTTP, or changes the command's output or exit status.
pub fn record_invocation(cli: &Cli) {
    if cli.dry_run || (cli.json && cli.command.is_none()) {
        return;
    }
    let Some(command) = category(cli.command.as_ref()) else {
        return;
    };
    let Ok((client, policy)) = client() else {
        return;
    };
    let explicit = cli.project.clone();
    let discover = cli.discovers_project();
    let client = client.with_project_context(move || {
        let project = explicit.or_else(|| {
            if !discover {
                return None;
            }
            crate::project::discover(&env::current_dir().ok()?)
                .ok()
                .flatten()
        });
        let origin = project
            .as_deref()
            .and_then(crate::git::telemetry_origin)
            .map_or(Origin::Unavailable, Origin::Present);
        RepositoryContext {
            origin,
            package_json: None,
        }
    });
    if reporting_blocked(&policy) {
        // Apply the SDK's local queue withdrawal even when networking is off.
        let _ = client.status();
    } else {
        let _ = client.record("cli.invocation", || {
            [("command".into(), command.into())].into()
        });
    }
    // A deletion can still be delivered under DO_NOT_TRACK.
    let _ = client.flush_in_background();
}

/// Enter the SDK's one-use worker before parsing or counting a CLI command.
pub fn run_worker(nonce: &str) -> bool {
    client().is_ok_and(|(client, _)| client.run_background_worker(nonce).is_ok())
}

pub(crate) fn settings(action: TelemetryAction, json: bool, dry_run: bool) -> DaloResult<()> {
    if dry_run {
        return Err(invalid(
            "use telemetry status or preview without --dry-run to inspect reporting",
        ));
    }
    let (client, policy) = client()?;
    match action {
        TelemetryAction::Status => {}
        TelemetryAction::Preview => {
            println!("{}", client.preview().map_err(sdk_error)?);
            return Ok(());
        }
        TelemetryAction::Enable => {
            if json || policy.ci || !io::stdin().is_terminal() || !io::stderr().is_terminal() {
                return Err(invalid(
                    "telemetry enable requires an interactive local terminal",
                ));
            }
            ensure_can_enable(&policy)?;
            let client = client.with_interaction(Interaction::Terminal);
            enable_locally(&client)?;
            return print_status(&client, false, policy.offline);
        }
        TelemetryAction::EnableCi => {
            if !policy.ci || policy.ci_revision.as_deref() != Some(CONSENT_REVISION) {
                return Err(invalid(
                    "telemetry enable-ci requires CI and APP_TRACKING_CI=telemetry-v1",
                ));
            }
            ensure_can_enable(&policy)?;
            client.set_opt_in(true).map_err(sdk_error)?;
            client.set_disabled(false).map_err(sdk_error)?;
        }
        TelemetryAction::Disable => client.set_opt_in(false).map_err(sdk_error)?,
        TelemetryAction::Delete => {
            let status = client.status().map_err(sdk_error)?;
            if !status.deletion_pending {
                client.delete().map_err(sdk_error)?;
            }
            // A scheduling failure leaves the explicit request pending.
            let _ = client.flush_in_background();
        }
    }
    print_status(&client, json, policy.offline)
}

fn ensure_can_enable(policy: &Policy) -> DaloResult<()> {
    if reporting_blocked(policy) {
        return Err(invalid(
            "usage reporting is disabled by the current environment",
        ));
    }
    Ok(())
}

fn enable_locally(client: &Client) -> DaloResult<()> {
    let status = client.status().map_err(sdk_error)?;
    if status.deletion_pending {
        return Err(invalid(
            "wait for the pending reporting deletion before enabling usage reporting",
        ));
    }
    if status.disabled || status.decision == app_tracking::client::OptInDecision::Declined {
        // The SDK preserves saved refusals. Only this explicit settings action
        // can reconsider one after showing the disclosure again.
        let mut stderr = io::stderr().lock();
        write!(
            stderr,
            "{DISCLOSURE}\n\nAllow optional usage reporting? [y/N] "
        )?;
        stderr.flush()?;
        drop(stderr);
        let mut answer = String::new();
        io::stdin().lock().take(256).read_line(&mut answer)?;
        if answer.ends_with('\n')
            && matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        {
            client.set_opt_in(true).map_err(sdk_error)?;
            client.set_disabled(false).map_err(sdk_error)?;
        }
    } else if let PermissionOutcome::Unavailable =
        client.request_opt_in(DISCLOSURE).map_err(sdk_error)?
        && status.opt_in
        && !status.collect_opt_in
    {
        return Err(invalid(
            "previous reporting permission has expired; run telemetry delete before enabling again",
        ));
    }
    Ok(())
}

fn print_status(client: &Client, json: bool, offline: bool) -> DaloResult<()> {
    let status = client.status().map_err(sdk_error)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&status)
                .map_err(|_| invalid("could not render reporting status"))?
        );
    } else {
        print_human_status(&status, offline);
    }
    Ok(())
}

fn print_human_status(status: &Status, offline: bool) {
    println!(
        "Usage reporting: {}",
        if status.collect_opt_in {
            "enabled"
        } else {
            "off"
        }
    );
    println!(
        "Saved choice: {}",
        match status.decision {
            app_tracking::client::OptInDecision::Undecided => "undecided",
            app_tracking::client::OptInDecision::Granted => "granted",
            app_tracking::client::OptInDecision::Declined => "declined or withdrawn",
        }
    );
    println!(
        "Queued batches: {} ({} open counters)",
        status.queued_batches, status.open_dimensions
    );
    if status.deletion_pending {
        println!(
            "Deletion pending. Keep the local reporting state until the service acknowledges it."
        );
    }
    if offline {
        println!("Offline mode: no reports or deletion requests are sent.");
    }
}

fn invalid(reason: &str) -> DaloError {
    DaloError::InvalidArgument {
        reason: reason.to_owned(),
    }
}

fn sdk_error(error: Error) -> DaloError {
    DaloError::StateError {
        reason: match error {
            Error::Busy => "usage reporting state is busy; retry this settings command",
            Error::PreviousIdentity => {
                "finish deleting the previous reporting identity before enabling reporting"
            }
            _ => "could not read or change usage reporting settings",
        }
        .to_owned(),
    }
}
